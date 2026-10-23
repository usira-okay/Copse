mod common;

use common::{TestSandbox, assert_code, copse, stdout};
use std::path::{Path, PathBuf};

fn tracked_repo(sandbox: &TestSandbox) -> PathBuf {
    let repo = sandbox.repo("repo");
    std::fs::write(repo.join("tracked.txt"), b"base\n").unwrap();
    sandbox.git(&repo, &["add", "tracked.txt"]);
    sandbox.git(&repo, &["commit", "-m", "tracked baseline"]);
    repo
}

fn assert_tracked_baseline(repo: &Path) {
    assert_eq!(std::fs::read(repo.join("tracked.txt")).unwrap(), b"base\n");
}

fn assert_index_unchanged(sandbox: &TestSandbox, repo: &Path) {
    assert!(sandbox.git(repo, &["diff", "--cached"]).stdout.is_empty());
}

#[test]
fn copy_requires_exactly_one_source_selector() {
    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    for arguments in [
        vec!["copy-changes"],
        vec!["copy-changes", "--from-path", ".", "--from-branch", "main"],
        vec!["copy-changes", "--from-path", ".", "--from-path", "."],
        vec![
            "copy-changes",
            "--from-branch",
            "main",
            "--from-branch",
            "main",
        ],
        vec!["copy-changes", "--from-branch", "main", "--force"],
    ] {
        assert_code(&copse(&repo, &arguments), 2);
        assert_tracked_baseline(&repo);
    }
}

#[test]
fn copy_rejects_unknown_same_foreign_and_non_root_sources() {
    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    let source = sandbox.worktree(&repo, "source", "source");
    let foreign = sandbox.repo("foreign");
    let subdir = source.join("subdir");
    std::fs::create_dir(&subdir).unwrap();
    for arguments in [
        vec!["copy-changes", "--from-branch", "missing"],
        vec!["copy-changes", "--from-branch", ""],
        vec!["copy-changes", "--from-branch", "main"],
        vec!["copy-changes", "--from-path", "."],
        vec!["copy-changes", "--from-path", "missing"],
        vec!["copy-changes", "--from-path", foreign.to_str().unwrap()],
        vec!["copy-changes", "--from-path", subdir.to_str().unwrap()],
    ] {
        assert_code(&copse(&repo, &arguments), 1);
        assert_tracked_baseline(&repo);
    }
}

#[test]
fn copy_combines_staged_and_unstaged_content_without_changing_source_or_index() {
    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    std::fs::write(repo.join("removed.txt"), b"remove me\n").unwrap();
    sandbox.git(&repo, &["add", "removed.txt"]);
    sandbox.git(&repo, &["commit", "-m", "deletion baseline"]);
    let source = sandbox.worktree(&repo, "source", "feature/source");
    std::fs::write(source.join("tracked.txt"), b"staged\n").unwrap();
    std::fs::write(source.join("新增 檔案.txt"), b"new staged file\n").unwrap();
    sandbox.git(&source, &["add", "tracked.txt", "新增 檔案.txt"]);
    std::fs::write(source.join("tracked.txt"), b"final content\n").unwrap();
    std::fs::remove_file(source.join("removed.txt")).unwrap();
    let source_diff = sandbox.git(&source, &["diff", "--binary", "HEAD"]);
    let source_index = sandbox.git(&source, &["diff", "--cached", "--binary"]);

    assert_code(
        &copse(&repo, &["copy-changes", "--from-branch", "feature/source"]),
        0,
    );

    assert_eq!(
        std::fs::read(repo.join("tracked.txt")).unwrap(),
        b"final content\n"
    );
    assert_eq!(
        std::fs::read(repo.join("新增 檔案.txt")).unwrap(),
        b"new staged file\n"
    );
    assert!(!repo.join("removed.txt").exists());
    assert_index_unchanged(&sandbox, &repo);
    assert_eq!(
        sandbox.git(&source, &["diff", "--binary", "HEAD"]).stdout,
        source_diff.stdout
    );
    assert_eq!(
        sandbox
            .git(&source, &["diff", "--cached", "--binary"])
            .stdout,
        source_index.stdout
    );
}

#[test]
fn copy_preserves_binary_bytes() {
    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    std::fs::write(repo.join("binary.bin"), [0, 1, 2, 255, 128, 0]).unwrap();
    sandbox.git(&repo, &["add", "binary.bin"]);
    sandbox.git(&repo, &["commit", "-m", "binary baseline"]);
    let source = sandbox.worktree(&repo, "source", "source");
    let expected = [0, 255, 254, 128, 127, 13, 10, 0, 3];
    std::fs::write(source.join("binary.bin"), expected).unwrap();
    assert_code(
        &copse(&repo, &["copy-changes", "--from-branch", "source"]),
        0,
    );
    assert_eq!(std::fs::read(repo.join("binary.bin")).unwrap(), expected);
    assert_index_unchanged(&sandbox, &repo);
}

#[test]
fn copy_large_patch_preserves_content_despite_diff_and_apply_configuration() {
    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    std::fs::write(repo.join(".gitattributes"), "tracked.txt diff=copse\n").unwrap();
    sandbox.git(&repo, &["add", ".gitattributes"]);
    sandbox.git(&repo, &["commit", "-m", "diff driver baseline"]);
    for (key, value) in [
        ("diff.external", "false"),
        ("diff.copse.textconv", "false"),
        ("diff.noprefix", "true"),
        ("diff.mnemonicPrefix", "true"),
        ("diff.relative", "true"),
        ("color.ui", "always"),
        ("apply.whitespace", "fix"),
    ] {
        sandbox.git(&repo, &["config", key, value]);
    }
    let source = sandbox.worktree(&repo, "source", "source");
    let expected: String = (0..16_000)
        .map(|index| format!("{index:06} 這行必須完整保留，包括結尾空白。  \n"))
        .collect();
    std::fs::write(source.join("tracked.txt"), &expected).unwrap();
    assert_code(
        &copse(&repo, &["copy-changes", "--from-branch", "source"]),
        0,
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("tracked.txt")).unwrap(),
        expected
    );
    assert_index_unchanged(&sandbox, &repo);
}

#[test]
fn copy_uses_final_net_change_when_staged_change_was_undone() {
    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    let source = sandbox.worktree(&repo, "source", "source");
    std::fs::write(source.join("tracked.txt"), b"staged only\n").unwrap();
    sandbox.git(&source, &["add", "tracked.txt"]);
    std::fs::write(source.join("tracked.txt"), b"base\n").unwrap();
    assert!(
        !sandbox
            .git(&source, &["diff", "--cached"])
            .stdout
            .is_empty()
    );
    assert_code(
        &copse(&repo, &["copy-changes", "--from-branch", "source"]),
        0,
    );
    assert!(
        sandbox
            .git(&repo, &["status", "--porcelain", "--untracked-files=all"])
            .stdout
            .is_empty()
    );
}

#[test]
fn copy_resolves_relative_source_from_effective_subdirectory_and_writes_to_root() {
    for directory_first in [true, false] {
        let sandbox = TestSandbox::new();
        let repo = tracked_repo(&sandbox);
        let destination = sandbox.worktree(&repo, "目的 空白", "destination");
        let subdir = destination.join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(repo.join("tracked.txt"), b"main worktree change\n").unwrap();
        let arguments = if directory_first {
            vec![
                "-C",
                subdir.to_str().unwrap(),
                "copy-changes",
                "--from-path",
                "../../repo",
            ]
        } else {
            vec![
                "copy-changes",
                "--from-path",
                "../../repo",
                "-C",
                subdir.to_str().unwrap(),
            ]
        };
        assert_code(&copse(&sandbox.root, &arguments), 0);
        assert_eq!(
            std::fs::read(destination.join("tracked.txt")).unwrap(),
            b"main worktree change\n"
        );
        assert!(!subdir.join("tracked.txt").exists());
        assert_index_unchanged(&sandbox, &destination);
    }
}

#[test]
fn copy_accepts_detached_source_by_exact_path() {
    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    let source = sandbox.root.join("detached source");
    sandbox.git(
        &repo,
        &["worktree", "add", "--detach", source.to_str().unwrap()],
    );
    std::fs::write(source.join("tracked.txt"), b"detached change\n").unwrap();
    assert_code(
        &copse(
            &repo,
            &["copy-changes", "--from-path", source.to_str().unwrap()],
        ),
        0,
    );
    assert_eq!(
        std::fs::read(repo.join("tracked.txt")).unwrap(),
        b"detached change\n"
    );
}

#[test]
fn copy_rejects_prunable_and_bare_sources() {
    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    let source = sandbox.worktree(&repo, "source", "source");
    std::fs::remove_dir_all(&source).unwrap();
    assert_code(
        &copse(&repo, &["copy-changes", "--from-branch", "source"]),
        1,
    );
    assert_tracked_baseline(&repo);

    let bare = sandbox.root.join("bare.git");
    sandbox.git(
        &sandbox.root,
        &[
            "clone",
            "--bare",
            repo.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    let destination = sandbox.worktree(&bare, "bare-linked", "destination");
    assert_code(
        &copse(
            &destination,
            &["copy-changes", "--from-path", bare.to_str().unwrap()],
        ),
        1,
    );
    assert_tracked_baseline(&destination);
}

#[test]
fn copy_excludes_untracked_by_default_and_includes_them_only_when_requested() {
    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    let source = sandbox.worktree(&repo, "source", "source");
    std::fs::create_dir(source.join("新目錄 空白")).unwrap();
    let relative = "新目錄 空白/未追蹤 檔案.bin";
    let content = [0, 255, 13, 10, 0, 128];
    std::fs::write(source.join(relative), content).unwrap();

    assert_code(
        &copse(&repo, &["copy-changes", "--from-branch", "source"]),
        0,
    );
    assert!(!repo.join(relative).exists());
    assert_code(
        &copse(
            &repo,
            &[
                "copy-changes",
                "--from-branch",
                "source",
                "--include-untracked",
            ],
        ),
        0,
    );
    assert_eq!(std::fs::read(repo.join(relative)).unwrap(), content);
    assert_eq!(std::fs::read(source.join(relative)).unwrap(), content);
    assert_index_unchanged(&sandbox, &repo);
}

#[cfg(unix)]
#[test]
fn copy_preserves_newlines_in_untracked_filenames() {
    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    let source = sandbox.worktree(&repo, "source", "source");
    let name = "換行\n檔案.txt";
    std::fs::write(source.join(name), b"untracked\n").unwrap();
    assert_code(
        &copse(
            &repo,
            &[
                "copy-changes",
                "--from-branch",
                "source",
                "--include-untracked",
            ],
        ),
        0,
    );
    assert_eq!(std::fs::read(repo.join(name)).unwrap(), b"untracked\n");
}

#[test]
fn copy_preflights_all_untracked_file_and_directory_collisions_before_any_write() {
    for destination_is_directory in [false, true] {
        let sandbox = TestSandbox::new();
        let repo = tracked_repo(&sandbox);
        let source = sandbox.worktree(&repo, "source", "source");
        std::fs::write(source.join("tracked.txt"), b"source change\n").unwrap();
        std::fs::write(source.join("a-safe.txt"), b"must not copy\n").unwrap();
        std::fs::write(source.join("z-collision"), b"must not overwrite\n").unwrap();
        if destination_is_directory {
            std::fs::create_dir(repo.join("z-collision")).unwrap();
            std::fs::write(repo.join("z-collision/retained.txt"), b"retained\n").unwrap();
        } else {
            std::fs::write(repo.join("z-collision"), b"retained\n").unwrap();
        }

        assert_code(
            &copse(
                &repo,
                &[
                    "copy-changes",
                    "--from-branch",
                    "source",
                    "--include-untracked",
                ],
            ),
            1,
        );

        assert_tracked_baseline(&repo);
        assert!(!repo.join("a-safe.txt").exists());
        let retained = if destination_is_directory {
            repo.join("z-collision/retained.txt")
        } else {
            repo.join("z-collision")
        };
        assert_eq!(std::fs::read(retained).unwrap(), b"retained\n");
        assert_index_unchanged(&sandbox, &repo);
    }
}

#[test]
fn copy_rejects_untracked_path_with_existing_file_as_destination_parent() {
    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    let source = sandbox.worktree(&repo, "source", "source");
    std::fs::write(source.join("tracked.txt"), b"source change\n").unwrap();
    std::fs::create_dir(source.join("nested")).unwrap();
    std::fs::write(source.join("nested/new.txt"), b"new\n").unwrap();
    std::fs::write(repo.join("nested"), b"retained\n").unwrap();
    assert_code(
        &copse(
            &repo,
            &[
                "copy-changes",
                "--from-branch",
                "source",
                "--include-untracked",
            ],
        ),
        1,
    );
    assert_tracked_baseline(&repo);
    assert_eq!(std::fs::read(repo.join("nested")).unwrap(), b"retained\n");
}

#[cfg(unix)]
#[test]
fn copy_rejects_untracked_source_symlinks_before_applying_tracked_patch() {
    use std::os::unix::fs::symlink;

    for link_to_directory in [false, true] {
        let sandbox = TestSandbox::new();
        let repo = tracked_repo(&sandbox);
        let source = sandbox.worktree(&repo, "source", "source");
        let outside = sandbox.root.join("outside");
        if link_to_directory {
            std::fs::create_dir(&outside).unwrap();
            std::fs::write(outside.join("nested.txt"), b"outside\n").unwrap();
        } else {
            std::fs::write(&outside, b"outside\n").unwrap();
        }
        symlink(&outside, source.join("linked")).unwrap();
        std::fs::write(source.join("tracked.txt"), b"source change\n").unwrap();
        assert_code(
            &copse(
                &repo,
                &[
                    "copy-changes",
                    "--from-branch",
                    "source",
                    "--include-untracked",
                ],
            ),
            1,
        );
        assert_tracked_baseline(&repo);
        assert!(!repo.join("linked").exists());
    }
}

#[cfg(unix)]
#[test]
fn copy_rejects_existing_and_dangling_destination_symlinks_before_any_write() {
    use std::os::unix::fs::symlink;

    for dangling in [false, true] {
        let sandbox = TestSandbox::new();
        let repo = tracked_repo(&sandbox);
        let source = sandbox.worktree(&repo, "source", "source");
        let outside = sandbox.root.join("outside.txt");
        if !dangling {
            std::fs::write(&outside, b"retained\n").unwrap();
        }
        symlink(&outside, repo.join("new.txt")).unwrap();
        std::fs::write(source.join("tracked.txt"), b"source change\n").unwrap();
        std::fs::write(source.join("new.txt"), b"must not overwrite\n").unwrap();
        assert_code(
            &copse(
                &repo,
                &[
                    "copy-changes",
                    "--from-branch",
                    "source",
                    "--include-untracked",
                ],
            ),
            1,
        );
        assert_tracked_baseline(&repo);
        assert!(
            std::fs::symlink_metadata(repo.join("new.txt"))
                .unwrap()
                .is_symlink()
        );
        if dangling {
            assert!(!outside.exists());
        } else {
            assert_eq!(std::fs::read(&outside).unwrap(), b"retained\n");
        }
    }
}

#[cfg(unix)]
#[test]
fn copy_rejects_destination_symlink_parent_before_any_write() {
    use std::os::unix::fs::symlink;

    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    let source = sandbox.worktree(&repo, "source", "source");
    let outside = sandbox.root.join("outside");
    std::fs::create_dir(&outside).unwrap();
    symlink(&outside, repo.join("nested")).unwrap();
    std::fs::create_dir(source.join("nested")).unwrap();
    std::fs::write(source.join("nested/new.txt"), b"must not escape\n").unwrap();
    std::fs::write(source.join("tracked.txt"), b"source change\n").unwrap();
    assert_code(
        &copse(
            &repo,
            &[
                "copy-changes",
                "--from-branch",
                "source",
                "--include-untracked",
            ],
        ),
        1,
    );
    assert_tracked_baseline(&repo);
    assert!(!outside.join("new.txt").exists());
    assert!(
        std::fs::symlink_metadata(repo.join("nested"))
            .unwrap()
            .is_symlink()
    );
}

#[test]
fn ordinary_copy_conflict_preserves_destination_without_staging_or_untracked_copy() {
    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    let source = sandbox.worktree(&repo, "source", "source");
    std::fs::write(source.join("tracked.txt"), b"source change\n").unwrap();
    std::fs::write(source.join("new.txt"), b"must not copy\n").unwrap();
    std::fs::write(repo.join("tracked.txt"), b"destination change\n").unwrap();
    let before_status = sandbox.git(&repo, &["status", "--porcelain"]);

    assert_code(
        &copse(
            &repo,
            &[
                "copy-changes",
                "--from-branch",
                "source",
                "--include-untracked",
            ],
        ),
        1,
    );

    assert_eq!(
        std::fs::read(repo.join("tracked.txt")).unwrap(),
        b"destination change\n"
    );
    assert!(!repo.join("new.txt").exists());
    assert_eq!(
        sandbox.git(&repo, &["status", "--porcelain"]).stdout,
        before_status.stdout
    );
    assert_index_unchanged(&sandbox, &repo);
}

#[test]
fn three_way_conflict_requires_opt_in_and_preserves_conflict_state() {
    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    let source = sandbox.worktree(&repo, "source", "source");
    std::fs::write(source.join("tracked.txt"), b"source change\n").unwrap();
    std::fs::write(repo.join("tracked.txt"), b"destination change\n").unwrap();
    sandbox.git(&repo, &["add", "tracked.txt"]);
    sandbox.git(&repo, &["commit", "-m", "divergent destination"]);

    assert_code(
        &copse(&repo, &["copy-changes", "--from-branch", "source"]),
        1,
    );
    assert!(
        sandbox
            .git(&repo, &["ls-files", "--unmerged"])
            .stdout
            .is_empty()
    );
    assert_eq!(
        std::fs::read(repo.join("tracked.txt")).unwrap(),
        b"destination change\n"
    );

    assert_code(
        &copse(
            &repo,
            &["copy-changes", "--from-branch", "source", "--3way"],
        ),
        1,
    );

    assert!(
        !sandbox
            .git(&repo, &["ls-files", "--unmerged"])
            .stdout
            .is_empty()
    );
    let conflicted = std::fs::read_to_string(repo.join("tracked.txt")).unwrap();
    assert!(conflicted.contains("<<<<<<<"), "{conflicted}");
    assert!(conflicted.contains("destination change"), "{conflicted}");
    assert!(conflicted.contains("source change"), "{conflicted}");
    assert_eq!(
        std::fs::read(source.join("tracked.txt")).unwrap(),
        b"source change\n"
    );
}

#[test]
fn three_way_can_merge_changes_that_fail_ordinary_context_check() {
    let sandbox = TestSandbox::new();
    let repo = tracked_repo(&sandbox);
    let baseline = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\n";
    std::fs::write(repo.join("tracked.txt"), baseline).unwrap();
    sandbox.git(&repo, &["add", "tracked.txt"]);
    sandbox.git(&repo, &["commit", "-m", "merge baseline"]);
    let source = sandbox.worktree(&repo, "source", "source");
    std::fs::write(
        source.join("tracked.txt"),
        baseline.replace("two", "source two"),
    )
    .unwrap();
    std::fs::write(
        repo.join("tracked.txt"),
        baseline.replace("five", "destination five"),
    )
    .unwrap();
    sandbox.git(&repo, &["add", "tracked.txt"]);
    sandbox.git(&repo, &["commit", "-m", "destination context change"]);
    assert_code(
        &copse(&repo, &["copy-changes", "--from-branch", "source"]),
        1,
    );
    assert_code(
        &copse(
            &repo,
            &["copy-changes", "--from-branch", "source", "--3way"],
        ),
        0,
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("tracked.txt")).unwrap(),
        baseline
            .replace("two", "source two")
            .replace("five", "destination five")
    );
    assert!(stdout(&sandbox.git(&repo, &["ls-files", "--unmerged"])).is_empty());
}
