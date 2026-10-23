#![cfg(unix)]

#[path = "../src/test_support.rs"]
mod test_support;

use std::process::Command;
use test_support::TestSandbox;

const PTY_DRIVER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/interactive_cli.py");
const TARGET_BRANCH: &str = "feature/中文目標";
const OTHER_BRANCH: &str = "feature/other";

fn run_scenario(scenario: &str) {
    let sandbox = TestSandbox::new();
    let repository = sandbox.repo("互動測試 repo");
    let target = sandbox.worktree(&repository, "目標 worktree", TARGET_BRANCH);
    let other = sandbox.worktree(&repository, "other", OTHER_BRANCH);
    let output = Command::new("python3")
        .arg("-B")
        .arg(PTY_DRIVER)
        .arg(scenario)
        .arg(env!("CARGO_BIN_EXE_git_worktree_copse"))
        .arg(&repository)
        .arg(&target)
        .output()
        .expect("互動整合測試需要 Python 3 標準庫的 pty 支援");
    assert!(
        output.status.success(),
        "PTY 情境 {scenario} 失敗：\nstdout：{}\nstderr：{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for (worktree, branch) in [(target, TARGET_BRANCH), (other, OTHER_BRANCH)] {
        assert!(
            worktree.is_dir(),
            "互動取消後 worktree 不得消失：{worktree:?}"
        );
        sandbox.git(
            &repository,
            &["show-ref", "--verify", &format!("refs/heads/{branch}")],
        );
    }
}

#[test]
fn fuzzy_search_preserves_original_target_and_captured_stdout() {
    run_scenario("fuzzy_target");
}

#[test]
fn defaults_escape_and_interrupt_preserve_their_distinct_behaviors() {
    run_scenario("defaults_and_cancellation");
}

#[test]
fn enter_with_no_matching_choice_does_not_start_an_operation() {
    run_scenario("no_match");
}

#[test]
fn cancelling_delete_mode_keeps_worktrees_and_branches() {
    run_scenario("delete_mode_cancel");
}

#[test]
fn cancelling_final_delete_confirmation_keeps_worktrees_and_branches() {
    run_scenario("delete_confirmation_cancel");
}
