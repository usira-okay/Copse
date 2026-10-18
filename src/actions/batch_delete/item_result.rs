use super::{outcome::Outcome, target::DeleteTarget};

/// 保留每個選取目標的結果，供摘要與退出狀態使用。
#[derive(Debug)]
pub struct ItemResult {
    pub target: DeleteTarget,
    pub outcome: Outcome,
}
