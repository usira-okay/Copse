[English](README.md) | [繁體中文](README.zh-TW.md)

# Copse

一個透過互動式選單與方向鍵操作來快速管理 [git worktree](https://git-scm.com/docs/git-worktree) 的 CLI 工具。

## 功能

- **互動式選擇** — 使用方向鍵導覽、模糊搜尋分支/標籤、多選 worktree
- **create** — 從任意本地/遠端分支或標籤建立新的 worktree
- **delete** — 移除一個或多個 worktree（可選擇同時刪除分支）
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
# 執行檔位於 target/release/copse
```

## 使用方式

在任何 git 儲存庫內執行 `copse`：

```bash
copse
```

會出現互動式選單讓你選擇操作：

```
Select an action:
> create
  delete
  move-to
  copy-changes
```

### 選項

| 旗標 | 縮寫 | 說明 |
|------|------|------|
| `--verbose` | `-v` | 顯示詳細的除錯輸出 |
| `--absolute-path` | `-a` | 建立 worktree 時使用絕對路徑（預設：相對路徑） |

```bash
copse --verbose
copse -a
```

### 操作

#### create

1. 選擇分支或標籤（支援模糊搜尋）
   - 排序：本地分支 → 遠端分支 → 本地標籤 → 遠端標籤
2. 選擇直接建立 worktree，或先建立新分支
3. Worktree 建立於 `../<repoName>-worktree/<branch_name>`
   - 範例：儲存庫位於 `~/ProjectA`，分支 `feature-1` → `~/ProjectA-worktree/feature-1`
4. 建立後可選擇是否切換到新的 worktree

#### delete

1. 多選要刪除的 worktree（空白鍵切換選取，Enter 確認）
2. 選擇刪除模式：
   - 刪除 worktree，保留分支
   - 強制刪除 worktree，保留分支
   - 刪除 worktree 並刪除分支
   - 強制刪除 worktree 並刪除分支
3. 若 worktree 有未提交的變更會顯示警告
4. 執行前需要確認

#### move-to

1. 從清單中選擇一個 worktree
2. 確認切換
3. 輸出 `cd` 指令（以及供 shell 包裝腳本使用的 `COPSE_CD:<path>`）

#### copy-changes

1. 選擇要複製變更的來源 worktree
2. 以 diff 方式將變更套用到目前的 worktree
   - 若 diff 可以乾淨套用，則直接套用
   - 若有衝突，會嘗試三方合併並標記衝突
3. 可選擇同時複製未追蹤的檔案

### Shell 整合

`copse` 無法直接變更 shell 的工作目錄。`move-to` 和 `create` 操作會輸出 `COPSE_CD:<path>` 行，供 shell 包裝腳本使用。

Bash/Zsh 包裝腳本範例：

```bash
copse_wrapper() {
  local output
  output=$(copse "$@")
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
  $output = & copse @args
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
- Rust ≥ 1.85（edition 2024）

## 授權條款

以 [Apache License 2.0](LICENSE) 授權。
