# Worktree 建立路徑改以主 Worktree 為基準

- Status: Approved
- Date: 2026-07-15
- Author: Copilot CLI (with @arisu-91app)

## 背景 / 問題

`copse create` 目前使用「執行指令當下所在的 worktree」來計算新 worktree 的建立位置：

```rust
let repo_name = git::get_repo_name()?;   // 取自目前所在 worktree 的 git root
let repo_root = git::get_repo_root()?;   // 目前所在 worktree 的 git root
let worktree_path = build_worktree_path(&repo_root, &repo_name, &branch_name);
```

`get_repo_root()` 內部呼叫 `git rev-parse --show-toplevel`，在 linked worktree 內執行時會回傳
「該 linked worktree 自己的路徑」，而非原始（主）worktree 的路徑。因此：

- `repo_name` 會被誤判為目前所在 worktree 的資料夾名稱（例如 `feature_worktree-folder`），而不是真正的
  repo 名稱（`Copse`）。
- `repo_root.parent()` 會被誤判為目前所在 worktree 的上一層目錄。

結果是：只要不是在主 worktree（default branch 所在的原始 clone）底下執行 `create`，算出來的路徑就會用錯的
基準目錄與錯的 repo 名稱，且每多從一個 linked worktree 執行一次，巢狀路徑就會越疊越深。

## 目標

不論目前在哪一個 worktree 底下執行 `create`，新 worktree 都應該建立在：

```
<主 worktree 的上一層目錄>/worktree/<真正的 repoName>/<branch>
```

以目前這個 repo 為例：主 worktree 在 `/home/ari/SourceCode/Copse`，所以任何時候建立新 worktree，
都應該落在 `/home/ari/SourceCode/worktree/Copse/<branch>`，不會因為你目前站在
`/home/ari/SourceCode/worktree/Copse/feature_worktree-folder` 而跑到別的地方。

子路徑格式（`worktree/<repoName>/<branch>`）本身**不變**，只改變「基準目錄從哪裡算出來」。

## 設計決策（已與使用者確認）

1. **如何判斷「主 worktree」**：直接沿用 `git worktree list --porcelain` 的第一筆（`Worktree.is_main`），
   不另外偵測 repo 實際的 default branch 名稱。理由：與現有 `delete.rs` / `move_to.rs` /
   `copy_changes.rs` 判斷主 worktree 的方式一致，不需要額外處理「default branch 未被任何 worktree
   checkout」的 fallback 情境，維持 KISS。
2. **命名規則不變**：維持 `worktree/<repoName>/<branch>` 巢狀資料夾（而非完全攤平成
   `<repoName 上一層>/<branch>`），以避免同層有多個 repo 時發生資料夾撞名。
3. **顯示用的相對路徑維持以「目前所在目錄」為基準**：`create` 執行後印出的
   `Worktree will be created at: <path>`（非 `--absolute-path` 模式）字樣，其相對路徑仍以使用者
   目前實際所在目錄計算，方便直接複製貼上 `cd` 指令；只有「實際建立的絕對路徑」改以主 worktree 為準。

## 實作方案

### `src/git.rs` 新增

```rust
/// 從 worktree 清單中找出主 worktree（純函式，方便單元測試，不需要真的呼叫 git）。
fn find_main_worktree(worktrees: &[Worktree]) -> Option<&Worktree> {
    worktrees.iter().find(|wt| wt.is_main)
}

/// 取得主 worktree 的根目錄路徑，無論目前在哪個 worktree 底下執行都一樣。
pub fn get_main_worktree_root(verbose: bool) -> Result<PathBuf> {
    let worktrees = list_worktrees(verbose)?;
    let main = find_main_worktree(&worktrees).context("Could not find main worktree")?;
    Ok(PathBuf::from(&main.path))
}
```

- `find_main_worktree` 為 private 純函式，只在 `git.rs` 內被 `get_main_worktree_root` 呼叫，並在
  `#[cfg(test)]` 中直接測試。
- `get_main_worktree_root` 重用既有 `list_worktrees()`，不新增額外的 git 子行程呼叫邏輯。

### `src/actions/create.rs` 修改

```rust
// Compute the worktree path: ../worktree/{repoName}/{branch_name}, anchored to
// the main worktree so the layout is consistent no matter which worktree
// `create` is run from.
let main_root = git::get_main_worktree_root(verbose)?;
let repo_name = main_root
    .file_name()
    .context("Could not determine repository name")?
    .to_string_lossy()
    .to_string();
let worktree_path = build_worktree_path(&main_root, &repo_name, &branch_name);

// Determine the path to use (relative or absolute). The relative display is
// still computed from the current directory (not main_root) so the printed
// `cd` hint is directly usable from wherever the user is standing.
let repo_root = git::get_repo_root()?;
let display_path = if absolute_path {
    worktree_path.clone()
} else {
    pathdiff_relative(&repo_root, &worktree_path)
};
```

`build_worktree_path` 本身的邏輯完全不變，只是呼叫端傳入的 `repo_root` / `repo_name` 來源改變。

### 清理

- `git::get_repo_name()` 在此變更後於全專案將不再有任何呼叫者（目前唯一呼叫者就是
  `create.rs` 原本那一行），視為此次變更直接造成的 dead code，一併移除。
- `git::get_repo_root()` 仍被 `main.rs`（verbose 模式印出目前所在 repo root）與上面新的
  `create.rs` 相對路徑顯示邏輯使用，維持不動。

## 測試計畫（TDD）

先寫失敗測試，再實作：

1. `src/git.rs` 新增 `#[cfg(test)] mod tests`，涵蓋 `find_main_worktree`：
   - 主 worktree 不是清單第一筆時仍可正確找到（用手動建構的 `Worktree` 資料，不呼叫真正的 git）
   - 清單中沒有任何 `is_main` 項目時回傳 `None`
2. `src/actions/create.rs` 既有測試（`builds_worktree_path_under_worktree_directory`、
   `sanitizes_branch_name_in_worktree_path`、`computes_relative_path_for_nested_worktree_directory`）
   邏輯不需修改，因為 `build_worktree_path` / `pathdiff_relative` 本身沒有變動，維持通過即可。

## 文件更新

`README.md`、`README.zh-TW.md` 的 `create` 操作說明（第 3 點路徑說明）補充一句，明確指出無論從哪個
worktree 執行，路徑基準都是主 worktree，行為一致。範例維持不變（主 repo 在 `~/ProjectA`、分支
`feature-1` → `~/worktree/ProjectA/feature-1`）。

## 驗證方式

1. `cargo build` 需成功無錯誤
2. `cargo test` 全數通過（含新增的 `find_main_worktree` 測試與既有 3 個測試）

## 範圍外（Out of scope）

- 不處理「主 worktree 目前簽出的分支不是 repo 實際 default branch」的情境（已與使用者確認，直接採用
  `is_main`，不額外偵測 default branch 名稱）
- 不變更 `delete` / `move-to` / `copy-changes` 的既有邏輯
- 不做版本號 bump（依既有慣例，版本號 bump 是獨立的 commit，不隨功能變更）
