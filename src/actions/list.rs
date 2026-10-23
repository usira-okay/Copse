use crate::{git, list_entry::ListEntry};
use anyhow::Result;
use std::io::{self, Write};
use std::path::Path;

const HEADING: &str = "路徑 | 分支 | 主 worktree | 目前位置 | bare | locked | prunable";
const DETACHED: &str = "detached HEAD";

/// 查詢 worktree 登錄，不查詢 dirty 狀態或變更 Git metadata。
pub fn run(directory: &Path, json: bool, verbose: bool) -> Result<()> {
    let entries = git::list_worktrees_at(directory, verbose)?;
    // 取最深的登錄根目錄，避免巢狀 worktree 將外層也標記為目前位置。
    let current = entries
        .iter()
        .filter(|entry| !entry.is_bare && directory.starts_with(&entry.path))
        .max_by_key(|entry| Path::new(&entry.path).components().count())
        .map(|entry| entry.path.clone());
    let entries: Vec<_> = entries
        .into_iter()
        .map(|entry| ListEntry {
            is_current: current.as_deref() == Some(entry.path.as_str()),
            path: entry.path,
            branch: if entry.branch.is_empty() {
                None
            } else {
                Some(entry.branch)
            },
            is_main: entry.is_main,
            is_bare: entry.is_bare,
            locked: entry.locked,
            prunable: entry.prunable,
        })
        .collect();
    let mut stdout = io::stdout().lock();
    if json {
        serde_json::to_writer(&mut stdout, &entries)?;
        writeln!(stdout)?;
    } else {
        writeln!(stdout, "{HEADING}")?;
        for entry in entries {
            writeln!(
                stdout,
                "{:?} | {:?} | {} | {} | {} | {:?} | {:?}",
                entry.path,
                entry.branch.as_deref().unwrap_or(DETACHED),
                entry.is_main,
                entry.is_current,
                entry.is_bare,
                entry.locked,
                entry.prunable
            )?;
        }
    }
    Ok(())
}
