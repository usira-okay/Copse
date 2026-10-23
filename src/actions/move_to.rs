use crate::{fuzzy_select::fuzzy_select, git, worktree_selection};
use anyhow::{Result, ensure};
use std::path::Path;

const SELECT_PROMPT: &str = "選擇要切換的 worktree";
const CANCELLED: &str = "已取消操作。";
const NO_TARGETS: &str = "沒有其他可切換的 worktree。";
const INVALID_TARGET: &str = "無法切換到 bare 或已失效的 worktree";
const CHANGE_HINT: &str = "切換目錄";
const CD_MARKER: &str = "COPSE_CD:";

/// 選取目標後共用與參數模式相同的路徑檢查及 shell 輸出。
pub fn run(verbose: bool) -> Result<()> {
    let entries = git::list_worktrees(verbose)?;
    if entries.len() <= 1 {
        println!("{NO_TARGETS}");
        return Ok(());
    }
    let labels: Vec<_> = entries
        .iter()
        .map(|entry| format!("{:?} [{}]", entry.path, entry.branch))
        .collect();
    let selected = fuzzy_select(SELECT_PROMPT, &labels)?;
    if let Some(index) = selected {
        execute(&entries[index])?;
    } else {
        println!("{CANCELLED}");
    }
    Ok(())
}

/// 驗證目標存在且可安全表示為單行，再交由 shell 包裝腳本切換。
pub fn execute(selected: &git::Worktree) -> Result<()> {
    ensure!(
        !selected.is_bare && selected.prunable.is_none(),
        INVALID_TARGET
    );
    let target = worktree_selection::canonical_path(Path::new(&selected.path))?;
    git::ensure_single_line_path(&target)?;
    println!("{CHANGE_HINT}：{target:?}");
    println!("{CD_MARKER}{}", target.display());
    Ok(())
}
