use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

use crate::{git, repository::Repository};

use super::{
    discovery::{canonical_path, ensure_within_root},
    item_result::ItemResult,
    mode::DeleteMode,
    outcome::Outcome,
    prepared_target::PreparedTarget,
    target::DeleteTarget,
};

const INCOMPLETE: &str = "批次刪除未全部完成，請查看結果摘要";
const PREFLIGHT_FAILED: &str = "刪除前檢查失敗，未移除任何 worktree";
const REPOSITORY_CHANGED: &str = "儲存庫歸屬或主 worktree 路徑已變更";
const TARGET_CHANGED: &str = "worktree 已不存在或不再屬於原儲存庫";
const BRANCH_CHANGED: &str = "worktree 分支已變更，請重新選取";
const PROTECTED: &str = "worktree 為主工作樹、啟動或有效工作目錄、bare、鎖定或失效項目，不能刪除";
const PATH_CHANGED: &str = "worktree 實際路徑已變更，請重新選取";
const DIRTY: &str = "一般模式保留有未提交變更的 worktree";
const NEW_CHANGES: &str = "最終確認後出現新的未提交變更，請重新確認再刪除";
const STATUS_FAILED: &str = "無法確認 worktree 的未提交變更";

fn verify_repository(repository: &Repository, verbose: bool) -> Result<()> {
    ensure!(
        canonical_path(&repository.root)? == repository.root,
        REPOSITORY_CHANGED
    );
    ensure!(
        git::get_repo_common_dir_at(&repository.root, verbose)? == repository.common_dir,
        REPOSITORY_CHANGED
    );
    Ok(())
}

fn inspect(
    target: &DeleteTarget,
    root: Option<&Path>,
    protected_dirs: &[PathBuf],
    verbose: bool,
) -> Result<bool> {
    verify_repository(&target.repository, verbose)?;
    let entries = git::list_worktrees_at(&target.repository.root, verbose)?;
    let main = entries.first().context(REPOSITORY_CHANGED)?;
    ensure!(
        main.is_main && canonical_path(Path::new(&main.path))? == target.repository.root,
        REPOSITORY_CHANGED
    );
    let entry = entries
        .iter()
        .find(|entry| entry.path == target.worktree.path)
        .context(TARGET_CHANGED)?;
    ensure!(
        !entry.is_main && !entry.is_bare && entry.locked.is_none() && entry.prunable.is_none(),
        PROTECTED
    );
    ensure!(entry.branch == target.worktree.branch, BRANCH_CHANGED);
    let actual = match root {
        Some(root) => ensure_within_root(Path::new(&entry.path), root)?,
        None => canonical_path(Path::new(&entry.path))?,
    };
    ensure!(actual == target.path, PATH_CHANGED);
    ensure!(
        !protected_dirs
            .iter()
            .any(|directory| directory.starts_with(&actual)),
        PROTECTED
    );
    ensure!(
        git::get_repo_common_dir_at(&actual, verbose)? == target.repository.common_dir,
        REPOSITORY_CHANGED
    );
    git::worktree_has_changes_at(Path::new(&entry.path), verbose).context(STATUS_FAILED)
}

/// 讀取最終確認所需的狀態，尚不執行任何寫入。
pub fn prepare(
    targets: &[DeleteTarget],
    root: Option<&Path>,
    protected_dirs: &[PathBuf],
    verbose: bool,
) -> Vec<PreparedTarget> {
    targets
        .iter()
        .cloned()
        .map(|target| PreparedTarget {
            status: inspect(&target, root, protected_dirs, verbose)
                .map_err(|error| format!("{error:#}")),
            target,
        })
        .collect()
}

/// 全部目標與探索都通過預檢後才允許開始刪除。
pub fn ensure_prepared(
    prepared: &[PreparedTarget],
    mode: DeleteMode,
    discovery_errors: &[String],
) -> Result<()> {
    let mut errors = discovery_errors.to_vec();
    for item in prepared {
        let reason = match &item.status {
            Ok(true) if !mode.force => Some(DIRTY),
            Err(reason) => Some(reason.as_str()),
            _ => None,
        };
        if let Some(reason) = reason {
            errors.push(format!("{:?}：{reason}", item.target.path));
        }
    }
    ensure!(
        errors.is_empty(),
        "{PREFLIGHT_FAILED}\n{}",
        errors.join("\n")
    );
    Ok(())
}

fn execute_one(
    prepared: &PreparedTarget,
    root: Option<&Path>,
    protected_dirs: &[PathBuf],
    mode: DeleteMode,
    verbose: bool,
) -> Outcome {
    let initially_dirty = match &prepared.status {
        Ok(dirty) => *dirty,
        Err(error) => return Outcome::RemoveFailed(error.clone()),
    };
    if initially_dirty && !mode.force {
        return Outcome::Skipped(DIRTY.to_string());
    }
    // 已確認會跳過的項目不升級為刪除；其餘項目在寫入前重新檢查。
    let dirty = match inspect(&prepared.target, root, protected_dirs, verbose) {
        Ok(dirty) => dirty,
        Err(error) => return Outcome::RemoveFailed(format!("{error:#}")),
    };
    if dirty && !mode.force {
        return Outcome::Skipped(DIRTY.to_string());
    }
    if dirty && !initially_dirty {
        return Outcome::Skipped(NEW_CHANGES.to_string());
    }
    match git::remove_worktree_at(
        &prepared.target.repository.root,
        Path::new(&prepared.target.worktree.path),
        mode.force,
        verbose,
    ) {
        Ok(()) => Outcome::Removed,
        Err(error) => Outcome::RemoveFailed(format!("{error:#}")),
    }
}

/// 重新驗證已確認的目標，逐項移除並彙整分支清理結果。
pub fn execute(
    prepared: &[PreparedTarget],
    root: Option<&Path>,
    protected_dirs: &[PathBuf],
    mode: DeleteMode,
    verbose: bool,
) -> Vec<ItemResult> {
    let mut results: Vec<_> = prepared
        .iter()
        .map(|item| ItemResult {
            target: item.target.clone(),
            outcome: execute_one(item, root, protected_dirs, mode, verbose),
        })
        .collect();

    if mode.delete_branch {
        let mut branches: BTreeMap<(PathBuf, String), Vec<usize>> = BTreeMap::new();
        for (index, result) in results.iter().enumerate() {
            if matches!(result.outcome, Outcome::Removed)
                && !result.target.worktree.branch.is_empty()
            {
                branches
                    .entry((
                        result.target.repository.common_dir.clone(),
                        result.target.worktree.branch.clone(),
                    ))
                    .or_default()
                    .push(index);
            }
        }
        // 同一分支可能被強制 checkout 到數個 worktree，必須全部移除後才清理一次。
        for ((_, branch), indices) in branches {
            let repository = &results[indices[0]].target.repository;
            let cleanup = verify_repository(repository, verbose).and_then(|()| {
                git::delete_branch_at(&repository.root, &branch, mode.force, verbose)
            });
            if let Err(error) = cleanup {
                for index in indices {
                    results[index].outcome = Outcome::BranchFailed(format!("{error:#}"));
                }
            }
        }
    }
    results
}

/// 所有已選取項目及探索皆完成時才回報成功。
pub fn ensure_complete(results: &[ItemResult], discovery_errors: &[String]) -> Result<()> {
    if !discovery_errors.is_empty()
        || results
            .iter()
            .any(|item| !matches!(item.outcome, Outcome::Removed))
    {
        bail!(INCOMPLETE);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::super::mode::MODES;
    use super::*;
    use crate::{git::Worktree, repository::Repository, test_support::TestSandbox};

    fn target(repo: &Path, path: &Path, branch: &str) -> DeleteTarget {
        DeleteTarget {
            repository: Repository {
                common_dir: canonical_path(&repo.join(".git")).unwrap(),
                root: canonical_path(repo).unwrap(),
            },
            worktree: Worktree {
                path: path.to_str().unwrap().to_string(),
                branch: branch.to_string(),
                is_main: path == repo,
                is_bare: false,
                locked: None,
                prunable: None,
            },
            path: canonical_path(path).unwrap(),
        }
    }

    fn branch_exists(repo: &Path, branch: &str) -> bool {
        Command::new("git")
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ])
            .current_dir(repo)
            .status()
            .unwrap()
            .success()
    }

    #[test]
    fn removes_same_named_branches_in_two_repositories_without_crossing_contexts() {
        let sandbox = TestSandbox::new();
        let repo_a = sandbox.repo("repos/alpha");
        let repo_b = sandbox.repo("repos/beta");
        let first = sandbox.worktree(&repo_a, "batch/alpha", "shared");
        let second = sandbox.worktree(&repo_b, "batch/beta", "shared");
        let root = sandbox.root.join("batch");
        let targets = vec![
            target(&repo_a, &first, "shared"),
            target(&repo_b, &second, "shared"),
        ];
        let prepared = prepare(&targets, Some(&root), std::slice::from_ref(&repo_a), false);

        let results = execute(
            &prepared,
            Some(&root),
            std::slice::from_ref(&repo_a),
            MODES[0],
            false,
        );

        assert!(
            results
                .iter()
                .all(|item| matches!(item.outcome, Outcome::Removed))
        );
        assert!(!first.exists() && !second.exists());
        assert!(branch_exists(&repo_a, "shared") && branch_exists(&repo_b, "shared"));
        assert!(repo_a.exists() && repo_b.exists());
        assert!(ensure_complete(&results, &[]).is_ok());
    }

    #[test]
    fn normal_mode_skips_untracked_changes_even_when_git_config_hides_them() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let path = sandbox.worktree(&repo, "batch/dirty", "dirty");
        sandbox.git(&repo, &["config", "status.showUntrackedFiles", "no"]);
        std::fs::write(path.join("untracked.txt"), "keep").unwrap();
        let root = sandbox.root.join("batch");
        let prepared = prepare(
            &[target(&repo, &path, "dirty")],
            Some(&root),
            std::slice::from_ref(&repo),
            false,
        );

        let results = execute(
            &prepared,
            Some(&root),
            std::slice::from_ref(&repo),
            MODES[2],
            false,
        );

        assert!(matches!(results[0].outcome, Outcome::Skipped(_)));
        assert!(path.join("untracked.txt").exists());
        assert!(branch_exists(&repo, "dirty"));
        assert!(ensure_complete(&results, &[]).is_err());
    }

    #[test]
    fn force_mode_removes_confirmed_changes_and_unmerged_branch() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let path = sandbox.worktree(&repo, "batch/dirty", "dirty");
        sandbox.git(&path, &["commit", "--allow-empty", "-m", "unmerged"]);
        std::fs::write(path.join("untracked.txt"), "discard").unwrap();
        let root = sandbox.root.join("batch");
        let prepared = prepare(
            &[target(&repo, &path, "dirty")],
            Some(&root),
            std::slice::from_ref(&repo),
            false,
        );
        assert!(matches!(prepared[0].status, Ok(true)));

        let results = execute(
            &prepared,
            Some(&root),
            std::slice::from_ref(&repo),
            MODES[3],
            false,
        );

        assert!(matches!(results[0].outcome, Outcome::Removed));
        assert!(!path.exists());
        assert!(!branch_exists(&repo, "dirty"));
    }

    #[test]
    fn reports_worktree_removed_but_unmerged_branch_preserved() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let path = sandbox.worktree(&repo, "batch/feature", "feature");
        sandbox.git(&path, &["commit", "--allow-empty", "-m", "unmerged"]);
        let root = sandbox.root.join("batch");
        let prepared = prepare(
            &[target(&repo, &path, "feature")],
            Some(&root),
            std::slice::from_ref(&repo),
            false,
        );

        let results = execute(
            &prepared,
            Some(&root),
            std::slice::from_ref(&repo),
            MODES[2],
            false,
        );

        assert!(matches!(results[0].outcome, Outcome::BranchFailed(_)));
        assert!(!path.exists());
        assert!(branch_exists(&repo, "feature"));
        assert!(ensure_complete(&results, &[]).is_err());
    }

    #[test]
    fn continues_after_disappeared_target_without_deleting_its_branch() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let first = sandbox.worktree(&repo, "batch/first", "first");
        let second = sandbox.worktree(&repo, "batch/second", "second");
        let root = sandbox.root.join("batch");
        let prepared = prepare(
            &[
                target(&repo, &first, "first"),
                target(&repo, &second, "second"),
            ],
            Some(&root),
            std::slice::from_ref(&repo),
            false,
        );
        sandbox.git(&repo, &["worktree", "remove", first.to_str().unwrap()]);

        let results = execute(
            &prepared,
            Some(&root),
            std::slice::from_ref(&repo),
            MODES[2],
            false,
        );

        assert!(matches!(results[0].outcome, Outcome::RemoveFailed(_)));
        assert!(matches!(results[1].outcome, Outcome::Removed));
        assert!(branch_exists(&repo, "first"));
        assert!(!branch_exists(&repo, "second"));
        assert!(!second.exists());
    }

    #[test]
    fn rechecks_new_changes_after_confirmation_in_both_modes() {
        for mode in [MODES[0], MODES[1]] {
            let sandbox = TestSandbox::new();
            let repo = sandbox.repo("repo");
            let path = sandbox.worktree(&repo, "batch/feature", "feature");
            let root = sandbox.root.join("batch");
            let prepared = prepare(
                &[target(&repo, &path, "feature")],
                Some(&root),
                std::slice::from_ref(&repo),
                false,
            );
            assert!(matches!(prepared[0].status, Ok(false)));
            std::fs::write(path.join("new.txt"), "keep").unwrap();

            let results = execute(
                &prepared,
                Some(&root),
                std::slice::from_ref(&repo),
                mode,
                false,
            );

            assert!(matches!(results[0].outcome, Outcome::Skipped(_)));
            assert!(path.join("new.txt").exists());
        }
    }

    #[test]
    fn rechecks_branch_and_lock_after_confirmation() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let first = sandbox.worktree(&repo, "batch/first", "first");
        let second = sandbox.worktree(&repo, "batch/second", "second");
        let root = sandbox.root.join("batch");
        let prepared = prepare(
            &[
                target(&repo, &first, "first"),
                target(&repo, &second, "second"),
            ],
            Some(&root),
            std::slice::from_ref(&repo),
            false,
        );
        sandbox.git(&first, &["switch", "-c", "replacement"]);
        sandbox.git(&repo, &["worktree", "lock", second.to_str().unwrap()]);

        let results = execute(
            &prepared,
            Some(&root),
            std::slice::from_ref(&repo),
            MODES[3],
            false,
        );

        assert!(
            results
                .iter()
                .all(|item| matches!(item.outcome, Outcome::RemoveFailed(_)))
        );
        assert!(first.exists() && second.exists());
        assert!(branch_exists(&repo, "first") && branch_exists(&repo, "second"));
    }

    #[test]
    fn defers_shared_branch_cleanup_until_all_selected_worktrees_are_removed() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let first = sandbox.worktree(&repo, "batch/first", "shared");
        let second = sandbox.root.join("batch/second");
        sandbox.git(
            &repo,
            &[
                "worktree",
                "add",
                "--force",
                second.to_str().unwrap(),
                "shared",
            ],
        );
        let root = sandbox.root.join("batch");
        let prepared = prepare(
            &[
                target(&repo, &first, "shared"),
                target(&repo, &second, "shared"),
            ],
            Some(&root),
            std::slice::from_ref(&repo),
            false,
        );

        let results = execute(
            &prepared,
            Some(&root),
            std::slice::from_ref(&repo),
            MODES[2],
            false,
        );

        assert!(
            results
                .iter()
                .all(|item| matches!(item.outcome, Outcome::Removed))
        );
        assert!(!first.exists() && !second.exists());
        assert!(!branch_exists(&repo, "shared"));
    }

    #[test]
    fn refuses_main_current_and_outside_root_targets() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let current = sandbox.worktree(&repo, "batch/current", "current");
        std::fs::create_dir_all(current.join("subdir")).unwrap();
        let outside = sandbox.worktree(&repo, "outside", "outside");
        let root = sandbox.root.join("batch");
        let prepared = prepare(
            &[
                target(&repo, &repo, "main"),
                target(&repo, &current, "current"),
                target(&repo, &outside, "outside"),
            ],
            Some(&root),
            &[current.join("subdir")],
            false,
        );

        let results = execute(
            &prepared,
            Some(&root),
            &[current.join("subdir")],
            MODES[3],
            false,
        );

        assert!(
            results
                .iter()
                .all(|item| matches!(item.outcome, Outcome::RemoveFailed(_)))
        );
        assert!(repo.exists() && current.exists() && outside.exists());
    }

    #[test]
    fn exit_status_requires_complete_selection_and_discovery() {
        assert!(ensure_complete(&[], &[]).is_ok());
        assert!(ensure_complete(&[], &["探索失敗".to_string()]).is_err());
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let path = sandbox.worktree(&repo, "batch/feature", "feature");
        for outcome in [
            Outcome::Skipped("dirty".to_string()),
            Outcome::RemoveFailed("failed".to_string()),
            Outcome::BranchFailed("partial".to_string()),
        ] {
            let results = vec![ItemResult {
                target: target(&repo, &path, "feature"),
                outcome,
            }];
            assert!(ensure_complete(&results, &[]).is_err());
        }
    }

    #[test]
    fn refuses_a_worktree_whose_git_marker_points_to_another_repository() {
        let sandbox = TestSandbox::new();
        let first_repo = sandbox.repo("first");
        let second_repo = sandbox.repo("second");
        let path = sandbox.worktree(&second_repo, "batch/feature", "feature");
        let selected = target(&second_repo, &path, "feature");
        std::fs::write(
            path.join(".git"),
            format!("gitdir: {}\n", first_repo.join(".git").display()),
        )
        .unwrap();
        let root = sandbox.root.join("batch");

        let prepared = prepare(
            &[selected],
            Some(&root),
            std::slice::from_ref(&second_repo),
            false,
        );

        assert!(prepared[0].status.is_err());
        let results = execute(
            &prepared,
            Some(&root),
            std::slice::from_ref(&second_repo),
            MODES[3],
            false,
        );
        assert!(matches!(results[0].outcome, Outcome::RemoveFailed(_)));
        assert!(path.exists() && first_repo.exists() && second_repo.exists());
        assert!(branch_exists(&second_repo, "feature"));
    }

    #[test]
    fn deletes_search_root_itself_then_cleans_branch_from_stable_main_context() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let path = sandbox.worktree(&repo, "exact root", "feature");
        let prepared = prepare(
            &[target(&repo, &path, "feature")],
            Some(&path),
            std::slice::from_ref(&repo),
            false,
        );

        let results = execute(
            &prepared,
            Some(&path),
            std::slice::from_ref(&repo),
            MODES[2],
            false,
        );

        assert!(matches!(results[0].outcome, Outcome::Removed));
        assert!(!path.exists() && repo.exists());
        assert!(!branch_exists(&repo, "feature"));
    }

    #[cfg(unix)]
    #[test]
    fn deletes_special_character_paths_without_trimming_or_unquoting_them() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo trailing \n");
        let path = sandbox.worktree(&repo, "batch/space \ttab\ntrailing \n", "feature");
        let root = sandbox.root.join("batch");
        let prepared = prepare(
            &[target(&repo, &path, "feature")],
            Some(&root),
            std::slice::from_ref(&repo),
            false,
        );

        let results = execute(
            &prepared,
            Some(&root),
            std::slice::from_ref(&repo),
            MODES[2],
            false,
        );

        assert!(
            matches!(results[0].outcome, Outcome::Removed),
            "{results:?}"
        );
        assert!(!path.exists() && repo.exists());
        assert!(!branch_exists(&repo, "feature"));
    }

    #[test]
    fn normal_mode_preserves_staged_and_unstaged_changes() {
        for staged in [false, true] {
            let sandbox = TestSandbox::new();
            let repo = sandbox.repo("repo");
            let path = sandbox.worktree(&repo, "batch/feature", "feature");
            std::fs::write(path.join("tracked.txt"), "before").unwrap();
            sandbox.git(&path, &["add", "tracked.txt"]);
            sandbox.git(&path, &["commit", "-m", "tracked file"]);
            std::fs::write(path.join("tracked.txt"), "after").unwrap();
            if staged {
                sandbox.git(&path, &["add", "tracked.txt"]);
            }
            let root = sandbox.root.join("batch");
            let prepared = prepare(
                &[target(&repo, &path, "feature")],
                Some(&root),
                std::slice::from_ref(&repo),
                false,
            );

            let results = execute(
                &prepared,
                Some(&root),
                std::slice::from_ref(&repo),
                MODES[0],
                false,
            );

            assert!(matches!(results[0].outcome, Outcome::Skipped(_)));
            assert_eq!(
                std::fs::read_to_string(path.join("tracked.txt")).unwrap(),
                "after"
            );
        }
    }

    #[test]
    fn removes_detached_worktree_without_deleting_an_unrelated_branch() {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let path = sandbox.worktree(&repo, "batch/detached", "feature");
        sandbox.git(&path, &["switch", "--detach"]);
        let root = sandbox.root.join("batch");
        let prepared = prepare(
            &[target(&repo, &path, "")],
            Some(&root),
            std::slice::from_ref(&repo),
            false,
        );

        let results = execute(
            &prepared,
            Some(&root),
            std::slice::from_ref(&repo),
            MODES[2],
            false,
        );

        assert!(matches!(results[0].outcome, Outcome::Removed));
        assert!(!path.exists());
        assert!(branch_exists(&repo, "feature"));
    }
}
