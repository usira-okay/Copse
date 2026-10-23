[English](README.md) | [繁體中文](README.zh-TW.md)

# Copse

A CLI tool to quickly control [git worktree](https://git-scm.com/docs/git-worktree) with interactive prompts and arrow-key navigation.

支援互動選單與非互動子命令，可用於終端操作及腳本自動化；詳細契約見[正體中文操作說明](README.zh-TW.md#參數模式)。

## Features

- **Interactive selection** — Navigate with arrow keys, fuzzy-search branches/tags, multi-select worktrees
- **create** — Create a new worktree from any local/remote branch or tag
- **list** — 列出 worktree，支援 `--json`
- **delete** — Remove one or more worktrees (with optional branch cleanup)
- **move-to** — Switch to another worktree
- **copy-changes** — Copy uncommitted changes between worktrees
- **batch-delete** — 從指定根目錄探索並刪除跨儲存庫的 worktree

## Installation

### 從 crates.io 安裝

```bash
cargo install git_worktree_copse --version 0.2.0 --locked
copse --help
```

crate 名稱維持 `git_worktree_copse`，0.2.0 起只提供 `copse` 執行檔。升級後請將既有腳本、捷徑與 shell wrapper 中的 `git_worktree_copse` 命令改為 `copse`；安裝指令仍使用原 crate 名稱。

未自訂安裝根目錄時，請確認 Cargo 的執行檔目錄已加入 `PATH`：Unix 預設為 `$HOME/.cargo/bin`，Windows 預設為 `%USERPROFILE%\.cargo\bin`（PowerShell：`$env:USERPROFILE\.cargo\bin`）。若設定 `CARGO_HOME`，則使用該目錄下的 `bin`；使用 `cargo install --root <目錄>` 時，則使用 `<目錄>/bin`。

### From source

```bash
cargo install --path . --locked
```

### Build manually

```bash
git clone https://github.com/usira-okay/Copse.git
cd Copse
cargo build --release --locked
# 執行檔位於 target/release/copse（Windows 為 copse.exe）
```

## Usage

Run `copse` inside any git repository:

```bash
copse
```

You will be presented with an interactive menu to select an action:

```
Select an action:
> 📂  create
  ❌  delete
  📍  move-to
  📋  copy-changes
  ❌  batch-delete
```

### Options

| Flag | Short | Description |
|------|-------|-------------|
| `--verbose` | `-v` | Show verbose debug output |
| `--absolute-path` | `-a` | Use absolute paths when creating worktrees (default: relative) |
| `-C <目錄>` | — | 指定有效工作目錄；可放在子命令前後 |

```bash
copse --verbose
copse -a
```

### 參數模式

不指定子命令時顯示互動選單；指定子命令後不會補問參數。

```bash
copse -C ~/ProjectA list --json
copse create feature/topic
copse create HEAD --new-branch feature/new
copse move-to --branch feature/topic
copse delete --branch feature/a --branch feature/b --yes
copse batch-delete --root ~/worktree --all --dry-run
copse batch-delete --root ~/worktree --all --yes
copse copy-changes --from-branch feature/topic --include-untracked --3way
```

| 子命令 | 用法 |
|--------|------|
| `create` | `<ref> [--new-branch <名稱>]` |
| `list` | `[--json]` |
| `move-to` | `--path <路徑>` 或 `--branch <分支>` |
| `delete` | `--path <路徑>…` 或 `--branch <分支>…`，加上 `--yes [--force] [--delete-branch]` |
| `batch-delete` | `--root <根目錄>`，搭配 `--path <路徑>…` 或 `--all`，以及 `--yes` 或 `--dry-run`；可加 `--force`、`--delete-branch` |
| `copy-changes` | `--from-path <路徑>` 或 `--from-branch <分支>`，可加 `--include-untracked`、`--3way` |

相對路徑以 `-C` 有效目錄為準，路徑 selector 須指向 worktree 根目錄，分支 selector 須為精確本地分支名稱。兩種 selector 不可混用；刪除可重複指定同類 selector，目標會去重。

`create` 使用本機已有的分支、遠端追蹤分支、標籤、`HEAD` 或 commit SHA，不會 fetch；有歧義時請提供完整 `refs/...`。建立位置固定為主 worktree 父目錄下的 `worktree/<repo>/<名稱>`，不提供 `--path`；既有目標不會被覆蓋。標籤、`HEAD`、SHA 預設使用 detached HEAD；遠端來源建立追蹤分支，本地同名分支已存在則失敗。

`list --json` 輸出陣列，欄位為 `path`、`branch`、`is_main`、`is_current`、`is_bare`、`locked`、`prunable`。路徑為絕對路徑，detached／bare 分支為 `null`，鎖定與失效原因為字串或 `null`；不查詢 dirty 狀態。

刪除會保護主 worktree、啟動及有效工作目錄所在的 worktree，以及 bare、locked、prunable 或無法確認的項目。所有目標通過預檢後才開始刪除；`--force` 允許丟棄變更，`--delete-branch` 才清理成功移除項目的本地分支。`batch-delete` 可在儲存庫外執行，`--dry-run` 不寫入，即使同時指定 `--yes` 也僅預覽；阻擋條件會回報失敗。

`copy-changes` 的來源與目的必須是同一儲存庫中的不同 worktree，目的固定為有效 worktree 根目錄。複製來源相對 `HEAD` 的最終淨變更，包含二進位內容，不保留 staged／unstaged 區分。未追蹤檔案須以 `--include-untracked` 明確啟用，並在任何 patch 寫入前檢查全部碰撞及符號連結。只有 `--3way` 允許三方合併；衝突時回報失敗並保留狀態。

退出碼：成功 `0`、參數錯誤 `2`、操作失敗 `1`。結果輸出至 stdout，錯誤與 debug 輸出至 stderr。執行期間部分失敗不會回復已完成的動作。Copse 與標準 Git 驗證不補問輸入；自訂 Git hooks 仍會執行，須自行確保不等待互動。

### Actions

互動操作需保留可輸入的終端，並讓 stderr 連接終端（TTY）；stdout 可由 shell wrapper 擷取，選單與 ANSI 控制碼不會混入 `COPSE_CD` 輸出。模糊搜尋支援中文與非連續字元；0.2.0 的選單外觀與搜尋結果排序可能與舊版不同。模糊搜尋選單按 Esc 可正常取消，按 Ctrl-C 則以非零退出碼中斷。

#### create

1. Select a branch or tag (with fuzzy search)
   - Sorted by: local branches → remote branches → local tags → remote tags
2. Choose how to create the worktree:
   - 🌿 Create a new branch from the selected ref, then create worktree (default)
   - 📂 Create worktree directly from the selected ref
3. The worktree is created at `../worktree/<repoName>/<branch_name>`, always anchored to the
    **main worktree** (the original clone), regardless of which worktree you run `create` from
   - Example: main worktree at `~/ProjectA` with branch `feature-1` → `~/worktree/ProjectA/feature-1`
4. 輸出 `COPSE_CD`，透過下方 shell wrapper 切換到新的 worktree

#### delete

1. Multi-select worktrees to delete (Space to toggle, Enter to confirm)
   - The main worktree and your current working directory worktree are excluded from the list
2. Choose a deletion mode:
   - ❌ Delete worktree(s) but keep branch(es)
   - ⚡ Force delete worktree(s) but keep branch(es)
   - ❌🌿 Delete worktree(s) and delete branch(es)
   - ⚡🌿 Force delete worktree(s) and delete branch(es)
3. Warnings are shown for worktrees with uncommitted changes
4. Confirmation prompt displays the selected worktrees/branches and the chosen deletion method

#### move-to

1. Select a worktree from the list
2. 輸出 `COPSE_CD:<path>`，供 shell wrapper 切換目錄

#### copy-changes

1. 選擇來源 worktree
2. 選擇是否包含未追蹤檔案，以及是否允許三方合併
3. 確認後使用與參數模式共用的預檢及複製流程

### Shell integration

`copse` cannot change the shell's working directory directly. The `move-to` and `create` actions output a `COPSE_CD:<path>` line that a shell wrapper can use.

Example wrapper for Bash/Zsh:

```bash
copse_wrapper() {
  local output copse_exit=0 line cd_path=''
  output=$(copse "$@") || copse_exit=$?
  printf '%s\n' "$output"
  if [ "$copse_exit" -ne 0 ]; then
    return "$copse_exit"
  fi
  while IFS= read -r line; do
    case "$line" in
      COPSE_CD:*) cd_path=${line#COPSE_CD:} ;;
    esac
  done <<< "$output"
  if [ -n "$cd_path" ]; then
    cd -- "$cd_path" || return
  fi
  return "$copse_exit"
}
alias cps='copse_wrapper'
```

Example wrapper for PowerShell:

```powershell
function Invoke-Copse {
  $output = & copse @args
  $copseExitCode = $LASTEXITCODE
  $output | Write-Output
  if ($copseExitCode -ne 0) {
    return
  }
  $cdLine = $output | Select-String '^COPSE_CD:(.+)$' | Select-Object -Last 1
  if ($cdLine) {
    Set-Location -LiteralPath $cdLine.Matches.Groups[1].Value -ErrorAction Stop
  }
  $global:LASTEXITCODE = $copseExitCode
}
Set-Alias -Name cps -Value Invoke-Copse
```

包裝腳本會保留失敗退出碼並留在原目錄；PowerShell 呼叫者可讀取 `$LASTEXITCODE`，需要結束腳本時使用 `exit $LASTEXITCODE`。`create`、`move-to` 會拒絕目的路徑中的 CR／LF，避免 `COPSE_CD` 被拆成多行。

## Requirements

- Git ≥ 2.36（所有操作統一使用 NUL 分隔的 worktree 清單）
- Rust ≥ 1.85 (edition 2024)

Git 2.36 僅為功能門檻，並非安全版本保證；例如 [Git 官方安全公告 GHSA-vwqx-4fm8-6qc9](https://github.com/git/git/security/advisories/GHSA-vwqx-4fm8-6qc9) 涵蓋多個仍符合此門檻的版本。請使用作業系統或 Git 發行商仍支援、已套用安全修補的版本；Copse 不會代為升級系統 Git。

## 開發與測試

先完成建置，再執行測試：

```bash
cargo build --release --locked
cargo test --locked
```

Unix 的互動整合測試需要 `python3`，僅使用 Python 標準函式庫建立 PTY；Windows 會略過這組測試。Python 並非安裝或執行 Copse 的相依工具。

## Releasing

Maintainers: see [docs/RELEASING.md](docs/RELEASING.md) for the crates.io release process (one-time setup plus the steps for every release).

## License

Licensed under the [Apache License 2.0](LICENSE).
