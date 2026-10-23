mod common;

use common::{TestSandbox, assert_code, copse, stdout};
use std::path::Path;

fn branch_name(sandbox: &TestSandbox, worktree: &Path) -> String {
    let output = sandbox.git(worktree, &["rev-parse", "--abbrev-ref", "HEAD"]);
    stdout(&output).trim().to_owned()
}

#[test]
fn help_and_version_work_outside_a_repository() {
    let sandbox = TestSandbox::new();
    for arguments in [["--help"], ["--version"]] {
        let output = copse(&sandbox.root, &arguments);
        assert_code(&output, 0);
        if arguments == ["--help"] {
            assert!(stdout(&output).contains("Usage: copse [OPTIONS] [COMMAND]"));
        }
    }
    let output = copse(&sandbox.root, &["list", "--help"]);
    assert_code(&output, 0);
    assert!(stdout(&output).contains("Usage: copse list [OPTIONS]"));
}

#[test]
fn interactive_menu_without_a_terminal_fails_without_changing_worktrees() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let before = sandbox.git(&repo, &["worktree", "list", "--porcelain"]);

    let output = copse(&repo, &[]);

    assert_code(&output, 1);
    assert!(String::from_utf8_lossy(&output.stderr).contains("模糊選單需要互動式終端"));
    assert!(!stdout(&output).contains("COPSE_CD:"));
    let after = sandbox.git(&repo, &["worktree", "list", "--porcelain"]);
    assert_eq!(before.stdout, after.stdout);
}

#[test]
fn missing_arguments_and_conflicting_selectors_are_usage_errors() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    for arguments in [
        vec!["create"],
        vec!["move-to"],
        vec!["delete"],
        vec!["copy-changes"],
        vec!["batch-delete"],
        vec!["move-to", "--path", ".", "--branch", "main"],
        vec!["unknown-operation"],
    ] {
        assert_code(&copse(&repo, &arguments), 2);
    }
}

#[test]
fn list_uses_global_directory_before_or_after_subcommand() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("儲存庫 空白");
    let linked = sandbox.worktree(&repo, "linked 空白", "feature/topic");
    for arguments in [
        vec!["-C", repo.to_str().unwrap(), "list"],
        vec!["list", "-C", repo.to_str().unwrap()],
    ] {
        let output = copse(&sandbox.root, &arguments);
        assert_code(&output, 0);
        assert!(stdout(&output).contains(repo.to_str().unwrap()));
        assert!(stdout(&output).contains(linked.to_str().unwrap()));
        assert!(stdout(&output).contains("feature/topic"));
    }
}

#[test]
fn list_json_contains_all_fields_and_preserves_registration_states() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let linked = sandbox.worktree(&repo, "linked", "topic");
    let prunable = sandbox.worktree(&repo, "prunable", "gone");
    let detached = sandbox.root.join("detached");
    sandbox.git(
        &repo,
        &["worktree", "add", "--detach", detached.to_str().unwrap()],
    );
    sandbox.git(
        &repo,
        &[
            "worktree",
            "lock",
            "--reason",
            "保留原因",
            linked.to_str().unwrap(),
        ],
    );
    std::fs::remove_dir_all(&prunable).unwrap();
    std::fs::create_dir(linked.join("subdir")).unwrap();
    let output = copse(
        &sandbox.root,
        &[
            "-v",
            "-C",
            linked.join("subdir").to_str().unwrap(),
            "list",
            "--json",
        ],
    );
    assert_code(&output, 0);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = json.as_array().unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry["is_current"] == true)
            .count(),
        1
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry["is_main"] == true)
            .count(),
        1
    );
    for entry in entries {
        let path = entry["path"].as_str().unwrap();
        assert!(Path::new(path).is_absolute());
        assert_eq!(entry.as_object().unwrap().len(), 7);
        assert_eq!(entry["is_main"], path == repo.to_str().unwrap());
        assert_eq!(entry["is_current"], path == linked.to_str().unwrap());
        assert_eq!(entry["is_bare"], false);
        match Path::new(path) {
            path if path == repo => {
                assert_eq!(entry["branch"], "main");
                assert!(entry["locked"].is_null());
                assert!(entry["prunable"].is_null());
            }
            path if path == linked => {
                assert_eq!(entry["branch"], "topic");
                assert_eq!(entry["locked"], "保留原因");
                assert!(entry["prunable"].is_null());
            }
            path if path == prunable => {
                assert_eq!(entry["branch"], "gone");
                assert!(entry["locked"].is_null());
                assert!(!entry["prunable"].as_str().unwrap().is_empty());
            }
            path if path == detached => {
                assert!(entry["branch"].is_null());
                assert!(entry["locked"].is_null());
                assert!(entry["prunable"].is_null());
            }
            path => panic!("非預期的 worktree：{}", path.display()),
        }
    }
}

#[test]
fn list_outside_a_repository_is_an_operation_error() {
    let sandbox = TestSandbox::new();
    assert_code(&copse(&sandbox.root, &["list"]), 1);
}

#[test]
fn create_local_branch_uses_fixed_sanitized_path_and_marker() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    sandbox.git(&repo, &["branch", "feature/topic"]);
    let output = copse(&repo, &["create", "feature/topic"]);
    assert_code(&output, 0);
    let target = sandbox.root.join("worktree/repo/feature_topic");
    assert!(target.is_dir());
    assert_eq!(branch_name(&sandbox, &target), "feature/topic");
    assert!(stdout(&output).contains(&format!("COPSE_CD:{}", target.display())));
}

#[test]
fn create_new_branch_from_linked_subdirectory_anchors_to_main_worktree() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let linked = sandbox.worktree(&repo, "other/location", "existing");
    std::fs::create_dir(linked.join("subdir")).unwrap();
    let output = copse(
        &sandbox.root,
        &[
            "-C",
            linked.join("subdir").to_str().unwrap(),
            "create",
            "HEAD",
            "--new-branch",
            "feature/new",
            "-a",
        ],
    );
    assert_code(&output, 0);
    let target = sandbox.root.join("worktree/repo/feature_new");
    assert_eq!(branch_name(&sandbox, &target), "feature/new");
    assert!(stdout(&output).contains(&format!("COPSE_CD:{}", target.display())));
}

#[test]
fn create_tag_head_and_commit_use_detached_head() {
    for reference_kind in ["tag", "HEAD", "full-sha", "short-sha"] {
        let sandbox = TestSandbox::new();
        let repo = sandbox.repo("repo");
        sandbox.git(&repo, &["tag", "release-v1"]);
        let sha = sandbox.git(&repo, &["rev-parse", "HEAD"]);
        let sha = stdout(&sha).trim();
        let reference = match reference_kind {
            "tag" => "release-v1",
            "full-sha" => sha,
            "short-sha" => &sha[..8],
            _ => "HEAD",
        };
        let output = copse(&repo, &["create", reference]);
        assert_code(&output, 0);
        let target = sandbox.root.join("worktree/repo").join(reference);
        assert_eq!(branch_name(&sandbox, &target), "HEAD");
    }
}

#[test]
fn create_remote_branch_sets_local_tracking_branch() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    sandbox.git(&repo, &["remote", "add", "origin", repo.to_str().unwrap()]);
    sandbox.git(&repo, &["update-ref", "refs/remotes/origin/topic", "HEAD"]);
    let output = copse(&repo, &["create", "origin/topic"]);
    assert_code(&output, 0);
    let target = sandbox.root.join("worktree/repo/topic");
    assert_eq!(branch_name(&sandbox, &target), "topic");
    let remote = sandbox.git(&repo, &["config", "branch.topic.remote"]);
    assert_eq!(stdout(&remote).trim(), "origin");
    let merge = sandbox.git(&repo, &["config", "branch.topic.merge"]);
    assert_eq!(stdout(&merge).trim(), "refs/heads/topic");
}

#[test]
fn create_remote_branch_rejects_existing_local_name() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    sandbox.git(&repo, &["remote", "add", "origin", repo.to_str().unwrap()]);
    sandbox.git(&repo, &["branch", "topic"]);
    sandbox.git(&repo, &["update-ref", "refs/remotes/origin/topic", "HEAD"]);
    assert_code(&copse(&repo, &["create", "origin/topic"]), 1);
    assert!(!sandbox.root.join("worktree/repo/topic").exists());
}

#[test]
fn create_rejects_ambiguous_short_ref_but_accepts_full_ref() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    sandbox.git(&repo, &["tag", "same"]);
    sandbox.git(&repo, &["commit", "--allow-empty", "-m", "branch commit"]);
    sandbox.git(&repo, &["branch", "same"]);
    assert_code(&copse(&repo, &["create", "same"]), 1);
    assert!(!sandbox.root.join("worktree/repo/same").exists());
    let output = copse(&repo, &["create", "refs/tags/same"]);
    assert_code(&output, 0);
    let target = stdout(&output)
        .lines()
        .find_map(|line| line.strip_prefix("COPSE_CD:"))
        .unwrap();
    assert_eq!(branch_name(&sandbox, Path::new(target)), "HEAD");
    assert_eq!(
        sandbox
            .git(Path::new(target), &["rev-parse", "HEAD"])
            .stdout,
        sandbox.git(&repo, &["rev-parse", "refs/tags/same"]).stdout
    );
    sandbox.git(&repo, &["worktree", "remove", target]);
    let output = copse(&repo, &["create", "refs/heads/same"]);
    assert_code(&output, 0);
    let target = stdout(&output)
        .lines()
        .find_map(|line| line.strip_prefix("COPSE_CD:"))
        .unwrap();
    let branch = sandbox.git(Path::new(target), &["symbolic-ref", "HEAD"]);
    assert_eq!(stdout(&branch).trim(), "refs/heads/same");
    assert_eq!(
        sandbox
            .git(Path::new(target), &["rev-parse", "HEAD"])
            .stdout,
        sandbox.git(&repo, &["rev-parse", "refs/heads/same"]).stdout
    );
}

#[test]
fn create_rejects_revision_expressions() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    sandbox.git(&repo, &["commit", "--allow-empty", "-m", "second"]);
    assert_code(&copse(&repo, &["create", "HEAD~1"]), 1);
    assert!(!sandbox.root.join("worktree/repo/HEAD~1").exists());
}

#[test]
fn create_rejects_checked_out_branch_unknown_ref_and_path_collision() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    assert_code(&copse(&repo, &["create", "main"]), 1);
    assert_code(&copse(&repo, &["create", "missing-ref"]), 1);
    sandbox.git(&repo, &["branch", "topic"]);
    let target = sandbox.root.join("worktree/repo/topic");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("keep.txt"), "原始內容").unwrap();
    assert_code(&copse(&repo, &["create", "topic"]), 1);
    assert_eq!(
        std::fs::read_to_string(target.join("keep.txt")).unwrap(),
        "原始內容"
    );
}

#[test]
fn move_to_path_and_branch_emit_same_absolute_marker() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let linked = sandbox.worktree(&repo, "linked 空白", "feature/topic");
    for arguments in [
        vec!["move-to", "--path", "../linked 空白"],
        vec!["move-to", "--branch", "feature/topic"],
    ] {
        let output = copse(
            &sandbox.root,
            &[vec!["-C", repo.to_str().unwrap()], arguments].concat(),
        );
        assert_code(&output, 0);
        assert!(stdout(&output).contains(&format!("COPSE_CD:{}", linked.display())));
    }
}

#[test]
fn move_to_unknown_target_is_an_operation_error() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    for arguments in [
        ["move-to", "--branch", "unknown"],
        ["move-to", "--path", "../unknown"],
    ] {
        let output = copse(&repo, &arguments);
        assert_code(&output, 1);
        assert!(!stdout(&output).contains("COPSE_CD:"));
    }
}

#[cfg(unix)]
#[test]
fn marker_paths_with_newlines_are_rejected_before_creation_or_output() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo\nunsafe");
    let output = copse(&repo, &["create", "HEAD", "--new-branch", "topic"]);
    assert_code(&output, 1);
    assert!(!sandbox.root.join("worktree/repo\nunsafe/topic").exists());
    assert!(!stdout(&output).contains("COPSE_CD:"));
    let output = copse(&repo, &["move-to", "--path", "."]);
    assert_code(&output, 1);
    assert!(!stdout(&output).contains("COPSE_CD:"));
}

#[cfg(unix)]
#[test]
fn create_rejects_newline_in_canonical_ancestor_before_creating_branch_or_worktree() {
    let sandbox = TestSandbox::new();
    let repo = sandbox.repo("repo");
    let unsafe_root = sandbox.root.join("unsafe\nancestor");
    std::fs::create_dir(&unsafe_root).unwrap();
    std::os::unix::fs::symlink(&unsafe_root, sandbox.root.join("worktree")).unwrap();
    let output = copse(&repo, &["create", "HEAD", "--new-branch", "topic"]);
    assert_code(&output, 1);
    assert!(!unsafe_root.join("repo/topic").exists());
    let branches = sandbox.git(
        &repo,
        &["for-each-ref", "--format=%(refname)", "refs/heads/topic"],
    );
    assert!(branches.stdout.is_empty());
    assert!(!stdout(&output).contains("COPSE_CD:"));
}
