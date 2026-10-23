use clap::{ArgGroup, Subcommand};
use std::path::PathBuf;

/// 指定子命令後只接受明確參數，不使用互動提示補足輸入。
#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// 從本機已有的來源建立 worktree，使用固定目錄規則。
    Create {
        /// 分支、遠端追蹤分支、標籤、HEAD 或 commit SHA。
        reference: String,
        /// 從來源建立指定的新分支。
        #[arg(long, value_name = "名稱")]
        new_branch: Option<String>,
    },
    /// 列出目前儲存庫的所有 worktree 登錄。
    List {
        /// 輸出供腳本解析的 JSON 陣列。
        #[arg(long)]
        json: bool,
    },
    /// 輸出 shell 切換目錄所需的 COPSE_CD 標記。
    #[command(group(ArgGroup::new("target").required(true).args(["path", "branch"])))]
    MoveTo {
        /// 精確指定 worktree 根目錄。
        #[arg(long, value_name = "路徑")]
        path: Option<PathBuf>,
        /// 精確指定 worktree 所使用的本地分支。
        #[arg(long, value_name = "分支")]
        branch: Option<String>,
    },
}
