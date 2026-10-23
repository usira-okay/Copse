use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use super::batch_delete::{self, DeleteMode, executor, target::DeleteTarget};
use crate::{git, repository::Repository, worktree_selection};

const CURRENT_DIRECTORY_FAILED: &str = "無法取得有效工作目錄";
const MAIN_MISSING: &str = "無法確認受保護的主 worktree";
const INVALID_SELECTION: &str = "必須擇一指定一組 worktree 路徑或本地分支";

fn repository(directory: &Path, entries: &[git::Worktree], verbose: bool) -> Result<Repository> {
    let main = entries
        .iter()
        .find(|entry| entry.is_main)
        .context(MAIN_MISSING)?;
    Ok(Repository {
        root: worktree_selection::canonical_path(Path::new(&main.path))?,
        common_dir: git::get_repo_common_dir_at(directory, verbose)?,
    })
}

fn target(repository: &Repository, worktree: &git::Worktree) -> Result<DeleteTarget> {
    Ok(DeleteTarget {
        repository: repository.clone(),
        path: worktree_selection::canonical_path(Path::new(&worktree.path))?,
        worktree: worktree.clone(),
    })
}

/// 先解析及預檢全部目標，再使用與批次操作相同的刪除與分支清理規則。
pub fn execute(
    repo_dir: &Path,
    protected_dirs: &[PathBuf],
    paths: &[PathBuf],
    branches: &[String],
    mode: DeleteMode,
    verbose: bool,
) -> Result<()> {
    ensure!(paths.is_empty() != branches.is_empty(), INVALID_SELECTION);
    let entries = git::list_worktrees_at(repo_dir, verbose)?;
    let repository = repository(repo_dir, &entries, verbose)?;
    let mut seen = HashSet::new();
    let mut targets = Vec::new();
    for (path, branch) in paths
        .iter()
        .map(|path| (Some(path.as_path()), None))
        .chain(branches.iter().map(|branch| (None, Some(branch.as_str()))))
    {
        let selected = worktree_selection::resolve(&entries, repo_dir, path, branch)?;
        let target = target(&repository, selected)?;
        if seen.insert(target.path.clone()) {
            targets.push(target);
        }
    }
    let prepared = executor::prepare(&targets, None, protected_dirs, verbose);
    executor::ensure_prepared(&prepared, mode, &[])?;
    let results = executor::execute(&prepared, None, protected_dirs, mode, verbose);
    batch_delete::print_results(&results, &[], mode.delete_branch);
    executor::ensure_complete(&results, &[])
}

/// 互動選取同一儲存庫的可刪除 worktree，再進入共用預檢與確認流程。
pub fn run(verbose: bool, protected_dirs: &[PathBuf]) -> Result<()> {
    let directory = std::env::current_dir().context(CURRENT_DIRECTORY_FAILED)?;
    let entries = git::list_worktrees_at(&directory, verbose)?;
    let repository = repository(&directory, &entries, verbose)?;
    let mut targets = Vec::new();
    for worktree in entries.iter().filter(|worktree| {
        !worktree.is_main
            && !worktree.is_bare
            && worktree.locked.is_none()
            && worktree.prunable.is_none()
    }) {
        let target = target(&repository, worktree)?;
        if !protected_dirs
            .iter()
            .any(|directory| directory.starts_with(&target.path))
        {
            targets.push(target);
        }
    }
    batch_delete::select_and_execute(&targets, None, protected_dirs, verbose)
}
