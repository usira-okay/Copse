use serde::Serialize;

/// 列表的穩定 JSON 契約；空分支代表 detached HEAD 或 bare 登錄。
#[derive(Serialize)]
pub struct ListEntry {
    pub path: String,
    pub branch: Option<String>,
    pub is_main: bool,
    pub is_current: bool,
    pub is_bare: bool,
    pub locked: Option<String>,
    pub prunable: Option<String>,
}
