/// 已解析的建立來源，將 ref 選擇與 Git 寫入分開以供互動及參數模式共用。
#[derive(Debug, Clone)]
pub struct WorktreeSource {
    /// 完整 ref；直接輸入 commit 時為已驗證的物件識別碼。
    pub reference: String,
    /// 來源指向的 commit，供 detached HEAD 與明確的新分支使用。
    pub commit: String,
    /// 沿用既有目錄配置的來源名稱，遠端分支已移除遠端前綴。
    pub directory_name: String,
    /// 要附著的本地分支；遠端來源為待建立的本地分支名稱。
    pub local_branch: Option<String>,
    /// 是否需要建立本地分支並追蹤遠端來源。
    pub track_remote: bool,
}
