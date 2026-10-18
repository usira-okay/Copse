use std::path::PathBuf;

/// 以共用 Git 目錄識別儲存庫，並保留不會被批次刪除的命令工作目錄。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    pub common_dir: PathBuf,
    pub root: PathBuf,
}
