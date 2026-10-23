use crate::cli_command::CliCommand;
use clap::Parser;
use std::path::PathBuf;

/// 互動選單與參數式操作共用的命令列入口。
#[derive(Parser, Debug)]
#[command(name = "copse", version, about = "以互動選單或參數管理 Git worktree")]
pub struct Cli {
    /// 顯示詳細診斷資訊。
    #[arg(short, long, global = true)]
    pub verbose: bool,
    /// 建立時以絕對路徑顯示目的位置。
    #[arg(short = 'a', long = "absolute-path", global = true)]
    pub absolute_path: bool,
    /// 使用指定工作目錄；所有相對路徑均以此目錄為基準。
    #[arg(short = 'C', global = true, value_name = "目錄")]
    pub directory: Option<PathBuf>,
    /// 省略子命令時開啟互動選單。
    #[command(subcommand)]
    pub command: Option<CliCommand>,
}
