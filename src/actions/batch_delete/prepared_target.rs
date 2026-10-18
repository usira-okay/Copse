use super::target::DeleteTarget;

/// 保存最終確認時的狀態，防止強制模式刪除確認後才出現的變更。
#[derive(Debug)]
pub struct PreparedTarget {
    pub target: DeleteTarget,
    /// `Ok(true)` 表示確認當時有未提交變更；錯誤項目禁止刪除。
    pub status: Result<bool, String>,
}
