use std::io::Read;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[path = "../../src/test_support.rs"]
mod test_support;

pub use test_support::TestSandbox;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// 關閉標準輸入並限制執行時間，驗證參數模式不會等待互動輸入。
pub fn copse(directory: &Path, arguments: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_copse"))
        .args(arguments)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let stdout = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let stderr = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() >= COMMAND_TIMEOUT {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("參數模式逾時，可能正在等待輸入：{arguments:?}");
        }
        thread::sleep(POLL_INTERVAL);
    };
    Output {
        status,
        stdout: stdout.join().unwrap(),
        stderr: stderr.join().unwrap(),
    }
}

/// 在斷言失敗時同時提供 stdout 與 stderr，便於診斷真實 CLI 行為。
pub fn assert_code(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout：{}\nstderr：{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// 讀取成功輸出的 UTF-8 文字。
pub fn stdout(output: &Output) -> &str {
    std::str::from_utf8(&output.stdout).unwrap()
}
