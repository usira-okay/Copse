use std::path::PathBuf;

use crate::{git::Worktree, repository::Repository};

/// 將刪除目標與其儲存庫及經確認的實際路徑綁定。
#[derive(Debug, Clone)]
pub struct DeleteTarget {
    pub repository: Repository,
    pub worktree: Worktree,
    pub path: PathBuf,
}
