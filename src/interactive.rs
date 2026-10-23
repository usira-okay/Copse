use crate::{actions, git};
use anyhow::Result;
use dialoguer::{FuzzySelect, theme::ColorfulTheme};
use std::path::PathBuf;

const ACTIONS: &[&str] = &[
    "📂  create",
    "❌  delete",
    "📍  move-to",
    "📋  copy-changes",
    "❌  batch-delete",
];
const ACTION_PROMPT: &str = "選擇操作";
const CANCELLED: &str = "已取消操作。";

/// 保留沒有子命令時的互動選單與取消行為。
pub fn run(verbose: bool, absolute_path: bool, protected_dirs: &[PathBuf]) -> Result<()> {
    git::get_repo_root()?;
    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt(ACTION_PROMPT)
        .items(ACTIONS)
        .default(0)
        .interact_opt()?;
    match selection {
        Some(0) => actions::create::run(verbose, absolute_path),
        Some(1) => actions::delete::run(verbose, protected_dirs),
        Some(2) => actions::move_to::run(verbose),
        Some(3) => actions::copy_changes::run(verbose),
        Some(4) => actions::batch_delete::run(verbose, protected_dirs),
        None => {
            println!("{CANCELLED}");
            Ok(())
        }
        _ => unreachable!(),
    }
}
