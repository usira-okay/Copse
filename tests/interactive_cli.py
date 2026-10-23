"""供 Rust 整合測試使用的有限 PTY 情境，stdout 獨立捕捉以模擬 shell wrapper。"""

import contextlib
import fcntl
import os
import pty
import select
import signal
import struct
import subprocess
import sys
import termios
import time

TIMEOUT_SECONDS = 10
POLL_SECONDS = 0.05
NO_MATCH_WAIT_SECONDS = 0.2
READ_SIZE = 65536
ROWS = 40
COLUMNS = 160
ESCAPE = b"\x1b"
INTERRUPT = b"\x03"
ENTER = b"\r"
ARROW_DOWN = b"\x1b[B"
ACTION_PROMPT = "選擇操作"
MOVE_PROMPT = "選擇要切換的 worktree"
REFERENCE_PROMPT = "選擇建立 worktree 的分支或標籤"
CREATE_MODE_PROMPT = "選擇建立 worktree 的方式"
BRANCH_PROMPT = "輸入新分支名稱"
DELETE_TARGET_PROMPT = "選取 worktree"
DELETE_MODE_PROMPT = "整批使用哪種刪除方式？"
DELETE_CONFIRM_PROMPT = "確定執行上述刪除？"
CANCELLED = "已取消操作。"
CD_MARKER = "COPSE_CD:"
PROMPTS = (
    ACTION_PROMPT,
    MOVE_PROMPT,
    REFERENCE_PROMPT,
    CREATE_MODE_PROMPT,
    BRANCH_PROMPT,
    DELETE_TARGET_PROMPT,
    DELETE_MODE_PROMPT,
    DELETE_CONFIRM_PROMPT,
    "選擇要複製變更的來源 worktree",
    "搜尋根目錄（",
)


class Session:
    """只提供本檔情境所需的輸入、等待提示與完成檢查。"""

    def __init__(self, master, process):
        """追蹤控制終端、子程序與本次互動的截止時間。"""
        self.master = master
        self.process = process
        self.transcript = bytearray()
        self.deadline = time.monotonic() + TIMEOUT_SECONDS

    def _read(self, timeout):
        ready, _, _ = select.select([self.master], [], [], timeout)
        if ready:
            self.transcript.extend(os.read(self.master, READ_SIZE))
        return bool(ready)

    def send(self, keys):
        """將完整按鍵序列送至控制終端。"""
        data = keys.encode() if isinstance(keys, str) else keys
        while data:
            data = data[os.write(self.master, data):]

    def expect(self, text):
        """等待指定提示，逾時或程序提早結束時回報終端紀錄。"""
        expected = text.encode()
        while expected not in self.transcript:
            assert time.monotonic() < self.deadline, f"等待提示逾時：{text}"
            received = self._read(POLL_SECONDS)
            assert received or self.process.poll() is None, f"提示出現前程序已結束：{text}"

    def assert_waiting(self):
        """給 Enter 一次處理時間，確認無匹配時仍等待選擇。"""
        until = time.monotonic() + NO_MATCH_WAIT_SECONDS
        while time.monotonic() < until:
            self._read(POLL_SECONDS)
            assert self.process.poll() is None, "沒有匹配項目時不應結束選單"
        for prompt in PROMPTS[1:]:
            assert prompt.encode() not in self.transcript, f"無匹配卻進入操作：{prompt}"

    def finish(self):
        """等待程序結束並檢查結果輸出沒有互動提示或 ANSI 序列。"""
        while self.process.poll() is None:
            assert time.monotonic() < self.deadline, "互動程序結束逾時"
            self._read(POLL_SECONDS)
        while self._read(0):
            pass
        output = self.process.communicate(timeout=TIMEOUT_SECONDS)[0].decode()
        assert "\x1b" not in output, f"stdout 含 ANSI 序列：{output!r}"
        for prompt in PROMPTS:
            assert prompt not in output, f"互動提示被寫入 stdout：{prompt}"
        return self.process.returncode, output


def _attach_terminal():
    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)


@contextlib.contextmanager
def session(binary, repository):
    """提供獨立控制終端，結束或斷言失敗時清理程序與檔案描述符。"""
    master, slave = pty.openpty()
    process = None
    terminal = None
    try:
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLUMNS, 0, 0))
        environment = os.environ | {"TERM": "xterm-256color", "NO_COLOR": "1", "CLICOLOR_FORCE": "0"}
        process = subprocess.Popen(
            [binary],
            cwd=repository,
            stdin=slave,
            stderr=slave,
            stdout=subprocess.PIPE,
            env=environment,
            # 此獨立測試程序不建立執行緒；fork 後須先取得控制終端再執行 CLI。
            preexec_fn=_attach_terminal,  # noqa: PLW1509
        )
        terminal = Session(master, process)
        yield terminal
    finally:
        if process is not None:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    # 程序可能剛好已退出；同步狀態後仍須回收子程序。
                    process.poll()
            process.wait(timeout=TIMEOUT_SECONDS)
            process.stdout.close()
        # 保留 slave 至程序結束，避免不同 Unix 平台將 PTY EOF 回報為 EIO。
        os.close(slave)
        os.close(master)
        if sys.exc_info()[0] is not None and terminal is not None:
            print(terminal.transcript.decode(errors="replace"), file=sys.stderr)


def fuzzy_target(binary, repository, target):
    """非連續與中文搜尋必須回到原始目標，且 shell marker 只出現一次。"""
    with session(binary, repository) as terminal:
        terminal.expect(ACTION_PROMPT)
        terminal.send("mvt\r")
        terminal.expect(MOVE_PROMPT)
        terminal.send("中標\r")
        code, output = terminal.finish()
        assert code == 0, output
        markers = [line for line in output.splitlines() if line.startswith(CD_MARKER)]
        assert markers == [CD_MARKER + os.path.realpath(target)], output


def defaults_and_cancellation(binary, repository, target):
    """驗證預設建立流程，以及 Esc 與 Ctrl-C 不同的結束狀態。"""
    for keys, success in [(ESCAPE, True), (INTERRUPT, False)]:
        with session(binary, repository) as terminal:
            terminal.expect(ACTION_PROMPT)
            terminal.send(keys)
            code, output = terminal.finish()
            assert (code == 0) == success, (code, output)
            assert CD_MARKER not in output, output
            if success:
                assert CANCELLED in output, output
    with session(binary, repository) as terminal:
        terminal.expect(ACTION_PROMPT)
        terminal.send(ENTER)
        terminal.expect(REFERENCE_PROMPT)
        terminal.send(ENTER)
        terminal.expect(CREATE_MODE_PROMPT)
        terminal.send(ENTER)
        terminal.expect(BRANCH_PROMPT)
        terminal.send(INTERRUPT)
        code, output = terminal.finish()
        assert code != 0 and CD_MARKER not in output, (code, output)


def no_match(binary, repository, target):
    """沒有搜尋結果時 Enter 不得偷偷選取預設操作。"""
    query = "zzzzzzzzzz"
    with session(binary, repository) as terminal:
        terminal.expect(ACTION_PROMPT)
        terminal.send(query)
        terminal.expect(query)
        terminal.send(ENTER)
        terminal.assert_waiting()
        terminal.send(ESCAPE)
        code, output = terminal.finish()
        assert code == 0 and CANCELLED in output, (code, output)
        assert CD_MARKER not in output, output


def _select_delete_target(terminal):
    terminal.expect(ACTION_PROMPT)
    terminal.send(ARROW_DOWN + ENTER)
    terminal.expect(DELETE_TARGET_PROMPT)
    terminal.send(b" " + ENTER)
    terminal.expect(DELETE_MODE_PROMPT)


def delete_mode_cancel(binary, repository, target):
    """取消刪除模式選單，交由 Rust 驗證目標及分支仍存在。"""
    with session(binary, repository) as terminal:
        _select_delete_target(terminal)
        terminal.send(ESCAPE)
        code, output = terminal.finish()
        assert code == 0 and CANCELLED in output, (code, output)


def delete_confirmation_cancel(binary, repository, target):
    """接受保守預設模式後取消最終確認，不得刪除 worktree 或分支。"""
    with session(binary, repository) as terminal:
        _select_delete_target(terminal)
        terminal.send(ENTER)
        terminal.expect(DELETE_CONFIRM_PROMPT)
        terminal.send(ESCAPE)
        code, output = terminal.finish()
        assert code == 0 and CANCELLED in output, (code, output)
        assert "刪除方式: ❌  刪除 worktree，保留本地分支" in output, output
        assert "強制刪除會永久丟棄" not in output, output


if __name__ == "__main__":
    scenario, binary, repository, target = sys.argv[1:]
    scenarios = {
        "fuzzy_target": fuzzy_target,
        "defaults_and_cancellation": defaults_and_cancellation,
        "no_match": no_match,
        "delete_mode_cancel": delete_mode_cancel,
        "delete_confirmation_cancel": delete_confirmation_cancel,
    }
    scenarios[scenario](binary, repository, target)
