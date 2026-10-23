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
    /// 刪除明確指定的 worktree；所有目標通過預檢後才開始。
    #[command(group(ArgGroup::new("targets").required(true).args(["paths", "branches"])))]
    Delete {
        /// 精確指定 worktree 根目錄，可重複使用。
        #[arg(long = "path", value_name = "路徑")]
        paths: Vec<PathBuf>,
        /// 精確指定本地分支，可重複使用。
        #[arg(long = "branch", value_name = "分支")]
        branches: Vec<String>,
        /// 確認執行刪除。
        #[arg(long, required = true)]
        yes: bool,
        /// 允許丟棄變更；搭配 --delete-branch 時也可刪除未合併分支。
        #[arg(long)]
        force: bool,
        /// 移除 worktree 後清理對應的本地分支。
        #[arg(long)]
        delete_branch: bool,
    },
    /// 從指定根目錄探索並刪除跨儲存庫 worktree。
    #[command(group(ArgGroup::new("targets").required(true).args(["paths", "all"])))]
    BatchDelete {
        /// 搜尋根目錄；相對路徑以有效工作目錄為準。
        #[arg(long, value_name = "根目錄")]
        root: PathBuf,
        /// 指定搜尋範圍內的 worktree 根目錄，可重複使用。
        #[arg(long = "path", value_name = "路徑")]
        paths: Vec<PathBuf>,
        /// 選取搜尋範圍內所有符合保護規則的候選。
        #[arg(long)]
        all: bool,
        /// 確認執行刪除；預覽時可以省略。
        #[arg(long, required_unless_present = "dry_run")]
        yes: bool,
        /// 僅預覽與檢查；即使同時指定 --yes 也不寫入。
        #[arg(long)]
        dry_run: bool,
        /// 允許丟棄變更；搭配 --delete-branch 時也可刪除未合併分支。
        #[arg(long)]
        force: bool,
        /// 移除 worktree 後清理對應的本地分支。
        #[arg(long)]
        delete_branch: bool,
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
