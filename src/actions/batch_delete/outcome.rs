/// 區分刪除完成、保護性跳過及已無法回復的部分完成。
#[derive(Debug)]
pub enum Outcome {
    Removed,
    Skipped(String),
    RemoveFailed(String),
    BranchFailed(String),
}
