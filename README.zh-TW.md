[English](README.md) | [繁體中文](README.zh-TW.md)

# Copse

一個透過互動式選單與方向鍵操作來快速管理 [git worktree](https://git-scm.com/docs/git-worktree) 的 CLI 工具。

## 功能

- **互動式選擇** — 使用方向鍵導覽、模糊搜尋分支/標籤、多選 worktree
- **create** — 從任意本地/遠端分支或標籤建立新的 worktree
- **delete** — 移除一個或多個 worktree（可選擇同時刪除分支）
- **batch-delete** — 從指定目錄探索跨 repo 的 worktree，合併多選並批次刪除
- **move-to** — 切換到另一個 worktree
- **copy-changes** — 在 worktree 之間複製未提交的變更

## 安裝

### 從原始碼安裝

```bash
cargo install --path .
```

### 手動建置

```bash
git clone https://github.com/usira-okay/Copse.git
cd Copse
cargo build --release
# 執行檔位於 target/release/git_worktree_copse
```

## 使用方式

在任何 git 儲存庫內執行 `git_worktree_copse`：

```bash
git_worktree_copse
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

```bash
git_worktree_copse --verbose
git_worktree_copse -a
```

### 操作

#### create

1. 選擇分支或標籤（支援模糊搜尋）
   - 排序：本地分支 → 遠端分支 → 本地標籤 → 遠端標籤
2. 選擇建立方式：
   - 🌿 從選取的 ref 建立新分支，再建立 worktree（預設）
   - 📂 直接從選取的 ref 建立 worktree
3. Worktree 建立於 `../worktree/<repoName>/<branch_name>`，且一律以**主 worktree**（原始 clone）為基準，
   不論你目前在哪個 worktree 底下執行 `create` 都一樣
   - 範例：主 worktree 位於 `~/ProjectA`，分支 `feature-1` → `~/worktree/ProjectA/feature-1`
4. 建立後自動切換到新的 worktree

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

此功能需要 **Git ≥2.36**，仍須在任一 Git repo 內啟動；既有操作的 Git 版本要求不變。

1. 在主選單選擇 `batch-delete`。
2. 確認或修改搜尋根目錄，預填主 worktree 父目錄下的 `worktree`，例如 `~/worktree`。相對路徑以啟動目錄為準；可清空輸入後按 Enter 取消。
3. 工具遞迴探索，但不跟隨目錄符號連結，遇到 repo／worktree 根目錄便停止往內；只納入實際位於搜尋根目錄內的項目。同一 repo 的多個入口會去重，清單列出 repo、分支與完整路徑。
   - 主 worktree、目前終端所在的 worktree（包含其子目錄）、locked 及失效登錄會排除並顯示原因。
   - 若部分目錄或 repo 無法讀取，會先顯示問題，仍可處理其他成功探索的項目。
4. 使用空白鍵多選、Enter 確認，並選擇整批共用的四種刪除模式，與 `delete` 相同。
   - 預設一般刪除並保留分支；一般模式跳過已暫存、未暫存或未追蹤的變更。
   - 強制模式會列明將丟棄的變更；若同時清理分支，也會強制刪除未合併的**本地**分支，不刪除遠端分支。
5. 檢視預設為否的最終確認。工具在刪除前重新檢查；無法可靠確認路徑、歸屬或狀態便不刪。若確認時乾淨的 worktree 隨後出現未提交變更，也會保留，需重新操作確認。
6. 逐項刪除，失敗後繼續；最後彙整完成、跳過、worktree 刪除失敗與分支清理失敗。分支只在對應 worktree 成功移除後清理，同 repo 同分支只清理一次。

取消或正常沒有候選時退出碼為 `0`；確認執行後，只要探索不完整或有任何選取項目未完成（包括跳過及 worktree 已刪但分支清理失敗），退出碼為 `1`。根目錄無效或無法讀取也會失敗。已完成的刪除不會回復。

第一版不自動解鎖、prune、登錄 repo，亦不提供非互動旗標。路徑與 Git 清單必須能可靠解碼為 UTF-8，否則會回報問題並保留項目。

#### move-to

1. 從清單中選擇一個 worktree
2. 選擇後直接切換到該 worktree（輸出 `COPSE_CD:<path>` 供 shell 包裝腳本使用）

#### copy-changes

1. 選擇要複製變更的來源 worktree
2. 以 diff 方式將變更套用到目前的 worktree
   - 若 diff 可以乾淨套用，則直接套用
   - 若有衝突，會嘗試三方合併並標記衝突
3. 可選擇同時複製未追蹤的檔案

### Shell 整合

`git_worktree_copse` 無法直接變更 shell 的工作目錄。`move-to` 和 `create` 操作會輸出 `COPSE_CD:<path>` 行，供 shell 包裝腳本使用。

Bash/Zsh 包裝腳本範例：

```bash
copse_wrapper() {
  local output
  output=$(git_worktree_copse "$@")
  echo "$output"
  local cd_line
  cd_line=$(echo "$output" | grep '^COPSE_CD:' | tail -1)
  if [ -n "$cd_line" ]; then
    cd "${cd_line#COPSE_CD:}" || return
  fi
}
alias cps='copse_wrapper'
```

PowerShell 包裝腳本範例：

```powershell
function Invoke-Copse {
  $output = & git_worktree_copse @args
  $output | Write-Output
  $cdLine = $output | Select-String '^COPSE_CD:(.+)$' | Select-Object -Last 1
  if ($cdLine) {
    Set-Location $cdLine.Matches.Groups[1].Value
  }
}
Set-Alias -Name cps -Value Invoke-Copse
```

## 需求

- Git ≥ 2.15（需支援 `git worktree`）
- `batch-delete` 額外要求 Git ≥ 2.36
- Rust ≥ 1.85（edition 2024）

## 發布

維護者請參考 [docs/RELEASING.md](docs/RELEASING.md)，內含發布到 crates.io 的一次性環境設定，以及每次發布的步驟。

## 授權條款

以 [Apache License 2.0](LICENSE) 授權。
