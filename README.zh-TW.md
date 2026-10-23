[English](README.md) | [繁體中文](README.zh-TW.md)

# Copse

一個支援互動式選單與命令列參數的 [git worktree](https://git-scm.com/docs/git-worktree) 管理工具。

## 功能

- **互動式選擇** — 使用方向鍵導覽、模糊搜尋分支/標籤、多選 worktree
- **create** — 從任意本地/遠端分支或標籤建立新的 worktree
- **list** — 顯示 worktree 清單，支援 JSON 輸出
- **delete** — 移除一個或多個 worktree（可選擇同時刪除分支）
- **batch-delete** — 從指定目錄探索跨 repo 的 worktree，合併多選並批次刪除
- **move-to** — 切換到另一個 worktree
- **copy-changes** — 在 worktree 之間複製未提交的變更

## 安裝

### 從 crates.io 安裝

```bash
cargo install git_worktree_copse --version 0.2.0 --locked
copse --help
```

crate 名稱維持 `git_worktree_copse`，0.2.0 起只提供 `copse` 執行檔。升級後請將既有腳本、捷徑與 shell wrapper 中的 `git_worktree_copse` 命令改為 `copse`；安裝指令仍使用原 crate 名稱。

未自訂安裝根目錄時，請確認 Cargo 的執行檔目錄已加入 `PATH`：Unix 預設為 `$HOME/.cargo/bin`，Windows 預設為 `%USERPROFILE%\.cargo\bin`（PowerShell：`$env:USERPROFILE\.cargo\bin`）。若設定 `CARGO_HOME`，則使用該目錄下的 `bin`；使用 `cargo install --root <目錄>` 時，則使用 `<目錄>/bin`。

### 從原始碼安裝

```bash
cargo install --path . --locked
```

### 手動建置

```bash
git clone https://github.com/usira-okay/Copse.git
cd Copse
cargo build --release --locked
# 執行檔位於 target/release/copse（Windows 為 copse.exe）
```

## 使用方式

在任何 git 儲存庫內執行 `copse`：

```bash
copse
```

會出現互動式選單讓你選擇操作：

```
Select an action:
> 📂  create
  ❌  delete
  📍  move-to
  📋  copy-changes
  ❌  batch-delete
```

### 選項

| 旗標 | 縮寫 | 說明 |
|------|------|------|
| `--verbose` | `-v` | 顯示詳細的除錯輸出 |
| `--absolute-path` | `-a` | 建立 worktree 時使用絕對路徑（預設：相對路徑） |
| `-C <目錄>` | — | 指定有效工作目錄；可放在子命令前後 |

```bash
copse --verbose
copse -a
```

### 參數模式

指定子命令後即使用非互動模式；缺少必要參數會直接回報錯誤。不指定子命令時保留互動選單。

```bash
# 在指定儲存庫查詢，stdout 僅輸出 JSON
copse -C ~/ProjectA list --json

# 從已有來源建立 worktree
copse create feature/topic
copse create HEAD --new-branch feature/new
copse create refs/remotes/origin/topic

# 選取 worktree；實際切換 shell 目錄需使用下方 wrapper
copse move-to --branch feature/topic
copse move-to --path ../worktree/ProjectA/feature_topic

# 必須明確確認刪除；同一種 selector 可重複
copse delete --branch feature/a --branch feature/b --yes
copse delete --path ../worktree/ProjectA/topic --yes --force --delete-branch

# 可在儲存庫外探索；預覽不需要 --yes
copse batch-delete --root ~/worktree --all --dry-run
copse batch-delete --root ~/worktree --path ~/worktree/ProjectA/topic --yes

# 將來源變更複製到有效工作目錄所屬 worktree 的根目錄
copse copy-changes --from-branch feature/topic
copse -C ~/ProjectA copy-changes --from-path ../worktree/ProjectA/topic --include-untracked --3way
```

| 子命令 | 參數 |
|--------|------|
| `create` | `<ref> [--new-branch <名稱>]` |
| `list` | `[--json]` |
| `move-to` | `--path <路徑>` 或 `--branch <分支>` |
| `delete` | `--path <路徑>…` 或 `--branch <分支>…`，加上 `--yes [--force] [--delete-branch]` |
| `batch-delete` | `--root <根目錄>`，搭配 `--path <路徑>…` 或 `--all`，以及 `--yes` 或 `--dry-run`；可加 `--force`、`--delete-branch` |
| `copy-changes` | `--from-path <路徑>` 或 `--from-branch <分支>`，可加 `--include-untracked`、`--3way` |

全域選項 `-C`、`-v`、`-a` 可置於子命令前後。所有相對路徑以 `-C` 指定的有效工作目錄為準；省略時以啟動目錄為準。路徑 selector 必須指向 worktree 根目錄，分支 selector 使用精確的本地分支名稱；路徑與分支兩種 selector 不可混用。找不到或無法唯一識別時失敗，刪除目標會去重。

#### 建立與列表契約

`create` 接受本機已有的本地分支、遠端追蹤分支、標籤、`HEAD`、完整或唯一縮寫的 commit SHA，不會執行 fetch。短名稱有歧義時須改用 `refs/heads/...`、`refs/remotes/...` 或 `refs/tags/...`。不接受 `HEAD~1` 等 revision expression。

- 本地分支直接掛載；標籤、`HEAD`、SHA 使用 detached HEAD。
- 遠端追蹤分支建立同名的本地追蹤分支；若本地分支已存在則失敗，可用 `--new-branch` 明確指定其他新分支。
- `--new-branch` 從指定來源建立新分支，名稱已存在時失敗。
- 路徑固定為主 worktree 父目錄下的 `worktree/<repo>/<名稱>`，名稱沿用既有字元轉換規則，例如 `/` 轉為 `_`；不提供 `--path` 覆寫建立位置。目標已存在時不會覆蓋，`-a` 僅影響顯示方式。
- `create` 與 `move-to` 的目的路徑不得含 CR 或 LF；建立前會檢查實際父目錄路徑，避免產生無法安全解析的 `COPSE_CD` 標記。

`list --json` 輸出下列欄位的陣列，不讀取 dirty 狀態：

```json
[
  {
    "path": "/home/user/ProjectA",
    "branch": "main",
    "is_main": true,
    "is_current": true,
    "is_bare": false,
    "locked": null,
    "prunable": null
  }
]
```

`path` 為絕對路徑；detached HEAD 與 bare 的 `branch` 為 `null`。`locked`、`prunable` 為 `null` 或 Git 提供的原因字串（可能為空字串）。`is_current` 以有效工作目錄所屬的 worktree 為準。失效登錄仍會出現在列表中。

#### 刪除與複製契約

刪除會保護主 worktree、啟動目錄及 `-C` 有效目錄所在的 worktree，以及 bare、locked、prunable、失蹤或無法確認狀態的項目。一般刪除遇到 staged、unstaged 或 untracked 變更即失敗；`--force` 允許丟棄這些變更。預設保留分支，`--delete-branch` 才在 worktree 成功移除後清理本地分支；搭配 `--force` 時也可刪除未合併分支。

所有指定目標通過預檢後才會開始刪除；任一預檢或探索錯誤都使整批停止。執行期間若部分項目失敗，仍會處理其他項目並回報退出碼 `1`，已完成的動作不會回復。`batch-delete --all` 沒有候選時成功，明確指定不存在或被排除的目標則失敗。`--dry-run` 會檢查相同的阻擋條件並回報失敗，但不刪除、不清理分支、不刷新 Git index；即使同時傳入 `--yes` 也僅預覽。

`copy-changes` 的來源與目的必須是同一儲存庫中的不同 worktree。目的固定為有效工作目錄所屬的 worktree 根目錄。複製內容是來源相對 `HEAD` 的最終淨變更，包含二進位檔案，不保留原來 staged／unstaged 的區分。預設只複製已追蹤內容；`--include-untracked` 才複製未追蹤檔案。

未追蹤檔案會在套用任何變更前全部檢查：來源須為一般檔案，目的檔案或目錄不得已存在，也不允許穿越符號連結。一般 patch 無法乾淨套用時直接失敗；只有指定 `--3way` 才嘗試三方合併，衝突時退出碼為 `1` 並保留衝突與 index 狀態，供後續處理。執行期間失敗不自動回復已寫入內容。

#### 腳本整合

退出碼 `0` 表示成功（以 Esc 正常取消亦為 `0`）、`2` 表示參數語法錯誤、`1` 表示操作失敗。結果寫入 stdout，錯誤及 `-v` 診斷寫入 stderr；`list --json` 的 stdout 可直接交給 JSON parser。

Copse 的參數模式與標準 Git 驗證不會補問互動輸入。Git hooks 仍會執行；自訂 hook 若會等待輸入，須由維護者自行調整。路徑與 Git 清單必須能可靠解碼為 UTF-8，否則回報錯誤。

### 互動操作

互動操作需保留可輸入的終端，並讓 stderr 連接終端（TTY）；stdout 可由 shell wrapper 擷取，選單與 ANSI 控制碼不會混入 `COPSE_CD` 輸出。模糊搜尋支援中文與非連續字元；0.2.0 的選單外觀與搜尋結果排序可能與舊版不同。模糊搜尋選單按 Esc 可正常取消，按 Ctrl-C 則以非零退出碼中斷。

#### create

1. 選擇分支或標籤（支援模糊搜尋）
   - 排序：本地分支 → 遠端分支 → 本地標籤 → 遠端標籤
2. 選擇建立方式：
   - 🌿 從選取的 ref 建立新分支，再建立 worktree（預設）
   - 📂 直接從選取的 ref 建立 worktree
3. Worktree 建立於 `../worktree/<repoName>/<branch_name>`，且一律以**主 worktree**（原始 clone）為基準，
   不論你目前在哪個 worktree 底下執行 `create` 都一樣
   - 範例：主 worktree 位於 `~/ProjectA`，分支 `feature-1` → `~/worktree/ProjectA/feature-1`
4. 建立後輸出 `COPSE_CD`，透過下方 shell wrapper 切換到新的 worktree

#### delete

1. 多選要刪除的 worktree（空白鍵切換選取，Enter 確認）
   - 主 worktree 和目前工作目錄所在的 worktree 會自動排除
2. 選擇刪除模式：
   - ❌ 刪除 worktree，保留分支
   - ⚡ 強制刪除 worktree，保留分支
   - ❌🌿 刪除 worktree 並刪除分支
   - ⚡🌿 強制刪除 worktree 並刪除分支
3. 若 worktree 有未提交的變更會顯示警告
4. 確認提示會列出選取的 worktree/分支以及刪除方式

#### batch-delete

互動選單須在任一 Git 儲存庫內啟動；參數模式 `batch-delete --root ...` 可在儲存庫外執行。所有操作均要求 **Git ≥2.36**。

1. 在主選單選擇 `batch-delete`。
2. 確認或修改搜尋根目錄，預填主 worktree 父目錄下的 `worktree`，例如 `~/worktree`。相對路徑以啟動目錄為準；可清空輸入後按 Enter 取消。
3. 工具遞迴探索，但不跟隨目錄符號連結，遇到 repo／worktree 根目錄便停止往內；只納入實際位於搜尋根目錄內的項目。同一 repo 的多個入口會去重，清單列出 repo、分支與完整路徑。
   - 主 worktree、目前終端所在的 worktree（包含其子目錄）、locked 及失效登錄會排除並顯示原因。
   - 若部分目錄或 repo 無法讀取，會回報問題並停止整批刪除。
4. 使用空白鍵多選、Enter 確認，並選擇整批共用的四種刪除模式，與 `delete` 相同。
   - 預設一般刪除並保留分支；一般模式發現已暫存、未暫存或未追蹤的變更時，整批預檢失敗。
   - 強制模式會列明將丟棄的變更；若同時清理分支，也會強制刪除未合併的**本地**分支，不刪除遠端分支。
5. 檢視預設為否的最終確認。工具在刪除前重新檢查；無法可靠確認路徑、歸屬或狀態便不刪。若確認時乾淨的 worktree 隨後出現未提交變更，也會保留，需重新操作確認。
6. 逐項刪除，失敗後繼續；最後彙整完成、跳過、worktree 刪除失敗與分支清理失敗。分支只在對應 worktree 成功移除後清理，同 repo 同分支只清理一次。

取消或正常沒有候選時退出碼為 `0`；確認執行後，只要探索不完整或有任何選取項目未完成（包括跳過及 worktree 已刪但分支清理失敗），退出碼為 `1`。根目錄無效或無法讀取也會失敗。已完成的刪除不會回復。

不自動解鎖、prune 或登錄 repo。非互動用法請參考上方參數模式。

#### move-to

1. 從清單中選擇一個 worktree
2. 選擇後輸出 `COPSE_CD:<path>`，供 shell 包裝腳本切換目錄

#### copy-changes

1. 選擇要複製變更的來源 worktree
2. 選擇是否複製未追蹤檔案，以及是否允許三方合併
3. 確認後，使用與參數模式相同的預檢與複製流程；錯誤會回報失敗

### Shell 整合

`copse` 無法直接變更 shell 的工作目錄。`move-to` 和 `create` 操作會輸出 `COPSE_CD:<path>` 行，供 shell 包裝腳本使用。

Bash/Zsh 包裝腳本範例：

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

PowerShell 包裝腳本範例：

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

包裝腳本會保留失敗退出碼並留在原目錄。PowerShell 腳本可在呼叫後檢查 `$LASTEXITCODE`，需要結束腳本時使用 `exit $LASTEXITCODE`。

## 需求

- Git ≥ 2.36（所有操作統一使用 NUL 分隔的 worktree 清單）
- Rust ≥ 1.85（edition 2024）

Git 2.36 僅為功能門檻，並非安全版本保證；例如 [Git 官方安全公告 GHSA-vwqx-4fm8-6qc9](https://github.com/git/git/security/advisories/GHSA-vwqx-4fm8-6qc9) 涵蓋多個仍符合此門檻的版本。請使用作業系統或 Git 發行商仍支援、已套用安全修補的版本；Copse 不會代為升級系統 Git。

## 開發與測試

先完成建置，再執行測試：

```bash
cargo build --release --locked
cargo test --locked
```

Unix 的互動整合測試需要 `python3`，僅使用 Python 標準函式庫建立 PTY；Windows 會略過這組測試。Python 並非安裝或執行 Copse 的相依工具。

## 發布

維護者請參考 [docs/RELEASING.md](docs/RELEASING.md)，內含發布到 crates.io 的一次性環境設定，以及每次發布的步驟。

## 授權條款

以 [Apache License 2.0](LICENSE) 授權。
