mod common;

use common::{TestSandbox, assert_code, copse, stdout};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const INDEX_REFRESH_DELAY: Duration = Duration::from_millis(20);

fn branch_exists(sandbox: &TestSandbox, repository: &Path, branch: &str) -> bool {
    !sandbox
        .git(
            repository,
            &[
                "for-each-ref",
                "--format=%(refname)",
                &format!("refs/heads/{branch}"),
            ],
        )
        .stdout
        .is_empty()
}

fn index_snapshot(sandbox: &TestSandbox, worktree: &Path) -> (Vec<u8>, SystemTime) {
    let index = sandbox.git(
        worktree,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    );
    let index = PathBuf::from(stdout(&index).trim());
    (
        std::fs::read(&index).unwrap(),
        std::fs::metadata(&index).unwrap().modified().unwrap(),
    )
}

fn commit_tracked_file(sandbox: &TestSandbox, repository: &Path) {
    std::fs::write(repository.join("tracked.txt"), "原始內容\n").unwrap();
    sandbox.git(repository, &["add", "tracked.txt"]);
    sandbox.git(repository, &["commit", "-m", "tracked fixture"]);
}

#[test]
fn delete_requires_confirmation_and_exclusive_selectors() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let linked = sandbox.worktree(&repo, "linked", "topic");
    for arguments in [
        vec!["delete", "--branch", "topic"],
        vec!["delete", "--path", linked.to_str().unwrap(), "--force"],
        vec!["delete", "--yes"],
        vec![
            "delete",
            "--path",
            linked.to_str().unwrap(),
            "--branch",
            "topic",
            "--yes",
        ],
    ] {
        assert_code(&copse(&repo, &arguments), 2);
        assert!(linked.exists());
    }
}

#[test]
fn delete_deduplicates_repeated_paths_or_branches_and_preserves_branch() {
    for by_branch in [false, true] {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let linked = sandbox.worktree(&repo, "linked 空白", "feature/topic");
        let arguments = if by_branch {
            vec![
                "delete",
                "--branch",
                "feature/topic",
                "--branch",
                "feature/topic",
                "--yes",
            ]
        } else {
            vec![
                "delete",
                "--path",
                linked.to_str().unwrap(),
                "--path",
                "../linked 空白",
                "--yes",
            ]
        };
        assert_code(&copse(&repo, &arguments), 0);
        assert!(!linked.exists());
        assert!(branch_exists(&sandbox, &repo, "feature/topic"));
    }
}

#[test]
fn delete_protects_main_original_and_effective_worktrees_before_any_removal() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let original = sandbox.worktree(&repo, "original", "original");
    let effective = sandbox.worktree(&repo, "effective", "effective");
    let clean = sandbox.worktree(&repo, "clean", "clean");
    std::fs::create_dir(original.join("subdir")).unwrap();
    std::fs::create_dir(effective.join("subdir")).unwrap();
    for protected in [&repo, &original, &effective] {
        let output = copse(
            &original.join("subdir"),
            &[
                "-C",
                effective.join("subdir").to_str().unwrap(),
                "delete",
                "--path",
                clean.to_str().unwrap(),
                "--path",
                protected.to_str().unwrap(),
                "--yes",
                "--force",
                "--delete-branch",
            ],
        );
        assert_code(&output, 1);
        assert!(repo.exists() && original.exists() && effective.exists() && clean.exists());
        assert!(branch_exists(&sandbox, &repo, "clean"));
    }
}

#[test]
fn delete_rejects_locked_prunable_and_unknown_targets_before_any_removal() {
    for state in ["locked", "prunable", "unknown"] {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let clean = sandbox.worktree(&repo, "clean", "clean");
        let blocked = sandbox.worktree(&repo, "blocked", "blocked");
        match state {
            "locked" => {
                sandbox.git(&repo, &["worktree", "lock", blocked.to_str().unwrap()]);
            }
            "prunable" => std::fs::remove_dir_all(&blocked).unwrap(),
            _ => {}
        }
        let branch = if state == "unknown" {
            "missing"
        } else {
            "blocked"
        };
        let before = sandbox.git(&repo, &["worktree", "list", "--porcelain", "-z"]);
        assert_code(
            &copse(
                &repo,
                &[
                    "delete", "--branch", "clean", "--branch", branch, "--yes", "--force",
                ],
            ),
            1,
        );
        assert!(clean.exists());
        assert_eq!(blocked.exists(), state != "prunable");
        assert_eq!(
            before.stdout,
            sandbox
                .git(&repo, &["worktree", "list", "--porcelain", "-z"])
                .stdout
        );
    }
}

#[test]
fn delete_rejects_unknown_path_before_any_removal() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let clean = sandbox.worktree(&repo, "clean", "clean");
    assert_code(
        &copse(
            &repo,
            &[
                "delete",
                "--path",
                clean.to_str().unwrap(),
                "--path",
                "../missing",
                "--yes",
            ],
        ),
        1,
    );
    assert!(clean.exists());
}

#[test]
fn delete_preserves_every_target_when_any_has_unconfirmed_changes() {
    for state in ["untracked", "unstaged", "staged"] {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        commit_tracked_file(&sandbox, &repo);
        let clean = sandbox.worktree(&repo, "clean", "clean");
        let dirty = sandbox.worktree(&repo, "dirty", "dirty");
        let changed_file = if state == "untracked" {
            "untracked.txt"
        } else {
            "tracked.txt"
        };
        std::fs::write(dirty.join(changed_file), "必須保留\n").unwrap();
        if state == "staged" {
            sandbox.git(&dirty, &["add", "tracked.txt"]);
        }
        sandbox.git(&repo, &["config", "status.showUntrackedFiles", "no"]);
        assert_code(
            &copse(
                &repo,
                &[
                    "delete",
                    "--branch",
                    "clean",
                    "--branch",
                    "dirty",
                    "--yes",
                    "--delete-branch",
                ],
            ),
            1,
        );
        assert!(clean.exists() && dirty.exists());
        assert_eq!(
            std::fs::read_to_string(dirty.join(changed_file)).unwrap(),
            "必須保留\n"
        );
        assert!(branch_exists(&sandbox, &repo, "clean") && branch_exists(&sandbox, &repo, "dirty"));
    }
}

#[test]
fn delete_force_discards_changes_and_only_cleans_branch_when_requested() {
    for delete_branch in [false, true] {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        let dirty = sandbox.worktree(&repo, "dirty", "topic");
        sandbox.git(&dirty, &["commit", "--allow-empty", "-m", "unmerged"]);
        std::fs::write(dirty.join("untracked.txt"), "明確授權丟棄").unwrap();
        let mut arguments = vec!["delete", "--branch", "topic", "--yes", "--force"];
        if delete_branch {
            arguments.push("--delete-branch");
        }
        assert_code(&copse(&repo, &arguments), 0);
        assert!(!dirty.exists());
        assert_eq!(branch_exists(&sandbox, &repo, "topic"), !delete_branch);
    }
}

#[test]
fn delete_reports_failure_after_removal_when_unmerged_branch_cannot_be_cleaned() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let linked = sandbox.worktree(&repo, "linked", "topic");
    sandbox.git(&linked, &["commit", "--allow-empty", "-m", "unmerged"]);
    assert_code(
        &copse(
            &repo,
            &["delete", "--branch", "topic", "--yes", "--delete-branch"],
        ),
        1,
    );
    assert!(!linked.exists());
    assert!(branch_exists(&sandbox, &repo, "topic"));
}

#[test]
fn delete_rejects_ambiguous_branch_without_removing_either_worktree() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let first = sandbox.worktree(&repo, "first", "shared");
    let second = sandbox.root.join("second");
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
    assert_code(&copse(&repo, &["delete", "--branch", "shared", "--yes"]), 1);
    assert!(first.exists() && second.exists());
}

#[test]
fn empty_branch_does_not_select_a_detached_worktree() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let detached = sandbox.root.join("detached");
    sandbox.git(
        &repo,
        &["worktree", "add", "--detach", detached.to_str().unwrap()],
    );
    for arguments in [
        vec!["move-to", "--branch", ""],
        vec!["delete", "--branch", "", "--yes"],
    ] {
        let output = copse(&repo, &arguments);
        assert_code(&output, 1);
        assert!(!stdout(&output).contains("COPSE_CD:"));
        assert!(detached.exists());
    }
}

#[test]
fn delete_rejects_unverifiable_repository_ownership_before_any_removal() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let foreign = sandbox.repo("foreign");
    let clean = sandbox.worktree(&repo, "clean", "clean");
    let changed = sandbox.worktree(&repo, "changed", "changed");
    std::fs::write(
        changed.join(".git"),
        format!("gitdir: {}\n", foreign.join(".git").display()),
    )
    .unwrap();
    assert_code(
        &copse(
            &repo,
            &[
                "delete", "--branch", "clean", "--branch", "changed", "--yes", "--force",
            ],
        ),
        1,
    );
    assert!(clean.exists() && changed.exists() && foreign.exists());
}

#[test]
fn delete_refuses_bare_repository() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let bare = sandbox.root.join("bare");
    sandbox.git(
        &sandbox.root,
        &[
            "clone",
            "--bare",
            repo.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    assert_code(
        &copse(&bare, &["delete", "--path", ".", "--yes", "--force"]),
        1,
    );
    assert!(bare.exists());
}

#[test]
fn batch_delete_requires_root_confirmation_and_exclusive_selection() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let linked = sandbox.worktree(&repo, "batch/linked", "topic");
    let root = sandbox.root.join("batch");
    for arguments in [
        vec!["batch-delete", "--root", root.to_str().unwrap(), "--all"],
        vec![
            "batch-delete",
            "--root",
            root.to_str().unwrap(),
            "--path",
            linked.to_str().unwrap(),
        ],
        vec!["batch-delete", "--root", root.to_str().unwrap(), "--yes"],
        vec!["batch-delete", "--all", "--yes"],
        vec![
            "batch-delete",
            "--root",
            root.to_str().unwrap(),
            "--all",
            "--path",
            linked.to_str().unwrap(),
            "--yes",
        ],
    ] {
        assert_code(&copse(&sandbox.root, &arguments), 2);
        assert!(linked.exists());
    }
}

#[test]
fn batch_delete_paths_use_effective_directory_outside_repositories_and_deduplicate() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let linked = sandbox.worktree(&repo, "batch/linked 空白", "topic");
    let control = sandbox.root.join("control");
    std::fs::create_dir(&control).unwrap();
    assert_code(
        &copse(
            &sandbox.root,
            &[
                "-C",
                "control",
                "batch-delete",
                "--root",
                "../batch",
                "--path",
                "../batch/linked 空白",
                "--path",
                linked.to_str().unwrap(),
                "--yes",
            ],
        ),
        0,
    );
    assert!(!linked.exists());
    assert!(branch_exists(&sandbox, &repo, "topic"));
}

#[test]
fn batch_all_cleans_same_named_branches_in_separate_repositories() {
    let sandbox = TestSandbox::new();
    let first_repo = sandbox.repo("repos/first");
    let second_repo = sandbox.repo("repos/second");
    let untouched_repo = sandbox.repo("repos/untouched");
    let first = sandbox.worktree(&first_repo, "batch/first", "shared");
    let second = sandbox.worktree(&second_repo, "batch/second", "shared");
    let untouched = sandbox.worktree(&untouched_repo, "outside", "shared");
    assert_code(
        &copse(
            &sandbox.root,
            &[
                "batch-delete",
                "--root",
                "batch",
                "--all",
                "--yes",
                "--delete-branch",
            ],
        ),
        0,
    );
    assert!(!first.exists() && !second.exists() && untouched.exists());
    assert!(!branch_exists(&sandbox, &first_repo, "shared"));
    assert!(!branch_exists(&sandbox, &second_repo, "shared"));
    assert!(branch_exists(&sandbox, &untouched_repo, "shared"));
}

#[test]
fn batch_all_with_no_candidates_succeeds() {
    let sandbox = TestSandbox::new();
    let root = sandbox.root.join("empty");
    std::fs::create_dir(&root).unwrap();
    for confirmation in ["--yes", "--dry-run"] {
        assert_code(
            &copse(
                &sandbox.root,
                &["batch-delete", "--root", "empty", "--all", confirmation],
            ),
            0,
        );
    }
}

#[test]
fn batch_dry_run_takes_precedence_over_yes_and_does_not_refresh_index() {
    for yes in [false, true] {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        commit_tracked_file(&sandbox, &repo);
        let linked = sandbox.worktree(&repo, "batch/linked", "topic");
        // 內容不變但檔案時間改變，未禁用 optional locks 的 git status 會刷新 index。
        std::thread::sleep(INDEX_REFRESH_DELAY);
        std::fs::write(linked.join("tracked.txt"), "原始內容\n").unwrap();
        let before = index_snapshot(&sandbox, &linked);
        let main_before = index_snapshot(&sandbox, &repo);
        let registrations = sandbox.git(&repo, &["worktree", "list", "--porcelain", "-z"]);
        let mut arguments = vec![
            "batch-delete",
            "--root",
            "batch",
            "--all",
            "--dry-run",
            "--delete-branch",
        ];
        if yes {
            arguments.extend(["--yes", "--force"]);
        }
        assert_code(&copse(&sandbox.root, &arguments), 0);
        assert!(linked.exists() && branch_exists(&sandbox, &repo, "topic"));
        assert_eq!(index_snapshot(&sandbox, &linked), before);
        assert_eq!(index_snapshot(&sandbox, &repo), main_before);
        assert_eq!(
            sandbox
                .git(&repo, &["worktree", "list", "--porcelain", "-z"])
                .stdout,
            registrations.stdout
        );
    }
}

#[test]
fn batch_dirty_preflight_and_dry_run_preserve_all_targets_and_indexes() {
    for dry_run in [false, true] {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        commit_tracked_file(&sandbox, &repo);
        let clean = sandbox.worktree(&repo, "batch/clean", "clean");
        let dirty = sandbox.worktree(&repo, "batch/dirty", "dirty");
        std::fs::write(dirty.join("tracked.txt"), "必須保留\n").unwrap();
        let clean_before = index_snapshot(&sandbox, &clean);
        let dirty_before = index_snapshot(&sandbox, &dirty);
        let mut arguments = vec![
            "batch-delete",
            "--root",
            "batch",
            "--all",
            "--yes",
            "--delete-branch",
        ];
        if dry_run {
            arguments.push("--dry-run");
        }
        assert_code(&copse(&sandbox.root, &arguments), 1);
        assert!(clean.exists() && dirty.exists());
        assert_eq!(
            std::fs::read_to_string(dirty.join("tracked.txt")).unwrap(),
            "必須保留\n"
        );
        assert_eq!(index_snapshot(&sandbox, &clean), clean_before);
        assert_eq!(index_snapshot(&sandbox, &dirty), dirty_before);
        assert!(branch_exists(&sandbox, &repo, "clean") && branch_exists(&sandbox, &repo, "dirty"));
    }
}

#[test]
fn batch_rejects_explicit_unknown_main_outside_locked_and_prunable_targets_atomically() {
    for state in ["unknown", "main", "outside", "locked", "prunable"] {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("batch/repo");
        let clean = sandbox.worktree(&repo, "batch/clean", "clean");
        let blocked = sandbox.worktree(&repo, "batch/blocked", "blocked");
        let outside = sandbox.worktree(&repo, "outside", "outside");
        let target = match state {
            "unknown" => sandbox.root.join("batch/missing"),
            "main" => repo.clone(),
            "outside" => outside.clone(),
            "locked" => {
                sandbox.git(&repo, &["worktree", "lock", blocked.to_str().unwrap()]);
                blocked.clone()
            }
            "prunable" => {
                std::fs::remove_dir_all(&blocked).unwrap();
                blocked.clone()
            }
            _ => unreachable!(),
        };
        assert_code(
            &copse(
                &sandbox.root,
                &[
                    "batch-delete",
                    "--root",
                    "batch",
                    "--path",
                    clean.to_str().unwrap(),
                    "--path",
                    target.to_str().unwrap(),
                    "--yes",
                    "--force",
                    "--delete-branch",
                ],
            ),
            1,
        );
        assert!(clean.exists() && repo.exists() && outside.exists());
        assert_eq!(blocked.exists(), state != "prunable");
        assert!(branch_exists(&sandbox, &repo, "clean"));
    }
}

#[test]
fn batch_protects_original_and_effective_directories_for_explicit_selection() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let original = sandbox.worktree(&repo, "batch/original", "original");
    let effective = sandbox.worktree(&repo, "batch/effective", "effective");
    let clean = sandbox.worktree(&repo, "batch/clean", "clean");
    std::fs::create_dir(original.join("subdir")).unwrap();
    std::fs::create_dir(effective.join("subdir")).unwrap();
    for target in [&original, &effective] {
        assert_code(
            &copse(
                &original.join("subdir"),
                &[
                    "-C",
                    effective.join("subdir").to_str().unwrap(),
                    "batch-delete",
                    "--root",
                    sandbox.root.join("batch").to_str().unwrap(),
                    "--path",
                    clean.to_str().unwrap(),
                    "--path",
                    target.to_str().unwrap(),
                    "--yes",
                    "--force",
                ],
            ),
            1,
        );
        assert!(original.exists() && effective.exists() && clean.exists());
    }
}

#[test]
fn batch_all_excludes_both_current_directories_main_locked_and_prunable_items() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("batch/repo");
    let original = sandbox.worktree(&repo, "batch/original", "original");
    let effective = sandbox.worktree(&repo, "batch/effective", "effective");
    let clean = sandbox.worktree(&repo, "batch/clean", "clean");
    let locked = sandbox.worktree(&repo, "batch/locked", "locked");
    let prunable = sandbox.worktree(&repo, "batch/prunable", "prunable");
    sandbox.git(&repo, &["worktree", "lock", locked.to_str().unwrap()]);
    std::fs::remove_dir_all(&prunable).unwrap();
    assert_code(
        &copse(
            &original,
            &[
                "-C",
                effective.to_str().unwrap(),
                "batch-delete",
                "--root",
                sandbox.root.join("batch").to_str().unwrap(),
                "--all",
                "--yes",
                "--force",
            ],
        ),
        0,
    );
    assert!(!clean.exists());
    assert!(repo.exists() && original.exists() && effective.exists() && locked.exists());
    assert!(branch_exists(&sandbox, &repo, "prunable"));
}

#[test]
fn batch_discovery_errors_prevent_removal_even_for_explicit_valid_target() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let clean = sandbox.worktree(&repo, "batch/clean", "clean");
    let corrupt = sandbox.root.join("batch/corrupt");
    std::fs::create_dir(&corrupt).unwrap();
    std::fs::write(
        corrupt.join(".git"),
        "gitdir: /missing/copse-test-repository\n",
    )
    .unwrap();
    for selection in [vec!["--all"], vec!["--path", clean.to_str().unwrap()]] {
        for confirmation in ["--yes", "--dry-run"] {
            let arguments = [
                vec!["batch-delete", "--root", "batch"],
                selection.clone(),
                vec![confirmation],
            ]
            .concat();
            assert_code(&copse(&sandbox.root, &arguments), 1);
            assert!(clean.exists());
            assert!(branch_exists(&sandbox, &repo, "clean"));
        }
    }
}

#[cfg(unix)]
#[test]
fn batch_rejects_explicit_paths_through_symlinks_inside_search_root() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let clean = sandbox.worktree(&repo, "batch/clean", "clean");
    let linked = sandbox.worktree(&repo, "batch/real/linked", "topic");
    std::os::unix::fs::symlink(
        sandbox.root.join("batch/real"),
        sandbox.root.join("batch/alias"),
    )
    .unwrap();
    for confirmation in ["--yes", "--dry-run"] {
        assert_code(
            &copse(
                &sandbox.root,
                &[
                    "batch-delete",
                    "--root",
                    "batch",
                    "--path",
                    clean.to_str().unwrap(),
                    "--path",
                    "batch/alias/linked",
                    confirmation,
                    "--force",
                ],
            ),
            1,
        );
        assert!(clean.exists() && linked.exists());
    }
}
