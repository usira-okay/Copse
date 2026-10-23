pub(super) mod discovery;
pub(super) mod executor;
pub(super) mod item_result;
mod mode;
mod outcome;
pub(super) mod prepared_target;
pub(super) mod target;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use console::style;
use dialoguer::{Confirm, Input, MultiSelect, theme::ColorfulTheme};

use crate::{fuzzy_select::fuzzy_select, git};
use discovery::canonical_path;
use item_result::ItemResult;
pub use mode::DeleteMode;
use mode::{MODE_LABELS, MODES};
use outcome::Outcome;
use prepared_target::PreparedTarget;
use target::DeleteTarget;

const CURRENT_DIRECTORY_FAILED: &str = "無法取得啟動目錄";
const MAIN_MISSING: &str = "無法找到主 worktree，不能決定預設搜尋位置";
const ROOT_PARENT_MISSING: &str = "主 worktree 沒有父目錄，無法決定預設搜尋位置";
const INVALID_ROOT_TEXT: &str = "搜尋根目錄不是有效的 UTF-8 路徑";
const ROOT_DIRECTORY: &str = "worktree";
const ROOT_PROMPT: &str = "搜尋根目錄（可修改；清空後 Enter 取消）";
const ROOT_CONTROL_PROMPT: &str = "搜尋根目錄（留空使用下列預設路徑）";
const CANCELLED: &str = "已取消操作。";
const SEARCHING: &str = "正在探索 worktree…";
const EXCLUDED: &str = "已排除的項目";
const UNKNOWN_TARGET: &str = "指定路徑不是搜尋範圍內可刪除的 worktree";
const SELECT_PROMPT: &str = "選取 worktree（空白鍵勾選，Enter 確認，Esc 取消）";
const MODE_PROMPT: &str = "整批使用哪種刪除方式？";
const NO_TARGETS: &str = "沒有可刪除的 worktree。";
const CONFIRMATION: &str = "即將處理的 worktree";
const CONFIRM_PROMPT: &str = "確定執行上述刪除？";
const FORCE_WARNING: &str = "強制刪除會永久丟棄所列項目的未提交變更。";
const FORCE_BRANCH_WARNING: &str = "此次也會強制清理本地分支，包含未合併的分支。";
const BRANCH_WARNING: &str = "此次也會清理已合併的本地分支。";
const METHOD: &str = "刪除方式";
const WILL_REMOVE: &str = "將刪除";
const WILL_DISCARD: &str = "將刪除並丟棄未提交變更";
const WILL_SKIP_DIRTY: &str = "整批停止：一般模式保留未提交變更";
const WILL_SKIP_ERROR: &str = "無法確認狀態，整批停止";
const REMOVED: &str = "已移除 worktree";
const REMOVED_WITH_BRANCH: &str = "已移除 worktree 與本地分支";
const SKIPPED: &str = "已跳過";
const REMOVE_FAILED: &str = "worktree 刪除失敗";
const BRANCH_FAILED: &str = "worktree 已刪，但分支清理失敗";
const SUMMARY: &str = "結果摘要";
const SUCCESS_COUNT: &str = "完成";
const SKIP_COUNT: &str = "跳過";
const FAILURE_COUNT: &str = "worktree 失敗";
const BRANCH_FAILURE_COUNT: &str = "分支清理失敗";
const DISCOVERY_FAILURE_COUNT: &str = "探索錯誤";
const DETACHED_HEAD: &str = "detached HEAD";

fn cancelled() -> Result<()> {
    println!("{}", style(CANCELLED).yellow());
    Ok(())
}

fn label(target: &DeleteTarget) -> String {
    let branch = if target.worktree.branch.is_empty() {
        DETACHED_HEAD
    } else {
        &target.worktree.branch
    };
    // Debug 格式只用於畫面，防止路徑的換行或終端控制字元破壞清單。
    format!(
        "{:?} | [{}] | {:?}",
        target.repository.root, branch, target.path
    )
}

fn print_messages(title: &str, messages: &[String]) {
    if !messages.is_empty() {
        println!("\n{}", style(title).yellow().bold());
        for message in messages {
            println!("  {}", message.escape_debug());
        }
    }
}

/// 將完成結果及摘要寫入標準輸出，失敗診斷則寫入標準錯誤。
pub(super) fn print_results(
    results: &[ItemResult],
    discovery_errors: &[String],
    delete_branch: bool,
) {
    let (mut removed, mut skipped, mut failed, mut branch_failed) = (0, 0, 0, 0);
    for result in results {
        let target = label(&result.target);
        match &result.outcome {
            Outcome::Removed => {
                removed += 1;
                let message = if delete_branch && !result.target.worktree.branch.is_empty() {
                    REMOVED_WITH_BRANCH
                } else {
                    REMOVED
                };
                println!("{}", style(format!("✓ {message}: {target}")).green());
            }
            Outcome::Skipped(reason) => {
                skipped += 1;
                eprintln!(
                    "{}",
                    style(format!("{SKIPPED}: {target}: {}", reason.escape_debug())).yellow()
                );
            }
            Outcome::RemoveFailed(reason) => {
                failed += 1;
                eprintln!(
                    "{}",
                    style(format!(
                        "{REMOVE_FAILED}: {target}: {}",
                        reason.escape_debug()
                    ))
                    .red()
                );
            }
            Outcome::BranchFailed(reason) => {
                branch_failed += 1;
                eprintln!(
                    "{}",
                    style(format!(
                        "{BRANCH_FAILED}: {target}: {}",
                        reason.escape_debug()
                    ))
                    .red()
                );
            }
        }
    }
    println!(
        "\n{SUMMARY}: {SUCCESS_COUNT} {} / {SKIP_COUNT} {} / {FAILURE_COUNT} {} / {BRANCH_FAILURE_COUNT} {} / {DISCOVERY_FAILURE_COUNT} {}",
        removed,
        skipped,
        failed,
        branch_failed,
        discovery_errors.len()
    );
}

fn print_prepared(prepared: &[PreparedTarget], mode: DeleteMode) {
    println!("\n{}", style(CONFIRMATION).bold());
    for item in prepared {
        let state = match &item.status {
            Ok(true) if mode.force => WILL_DISCARD.to_string(),
            Ok(true) => WILL_SKIP_DIRTY.to_string(),
            Ok(false) => WILL_REMOVE.to_string(),
            Err(reason) => format!("{WILL_SKIP_ERROR}: {}", reason.escape_debug()),
        };
        println!("  {} — {state}", label(&item.target));
    }
    if mode.force {
        println!("{}", style(FORCE_WARNING).red().bold());
    }
    if mode.delete_branch {
        println!(
            "{}",
            if mode.force {
                FORCE_BRANCH_WARNING
            } else {
                BRANCH_WARNING
            }
        );
    }
}

/// 單一儲存庫與跨儲存庫共用選取、最終確認及整批預檢。
pub(super) fn select_and_execute(
    targets: &[DeleteTarget],
    root: Option<&Path>,
    protected_dirs: &[PathBuf],
    verbose: bool,
) -> Result<()> {
    if targets.is_empty() {
        println!("{NO_TARGETS}");
        return Ok(());
    }
    let theme = ColorfulTheme::default();
    let labels: Vec<_> = targets.iter().map(label).collect();
    let selections = MultiSelect::with_theme(&theme)
        .with_prompt(SELECT_PROMPT)
        .items(&labels)
        .interact_opt()?;
    let selections = match selections {
        Some(selected) if !selected.is_empty() => selected,
        _ => return cancelled(),
    };
    let choice = fuzzy_select(MODE_PROMPT, &MODE_LABELS)?;
    let Some(choice) = choice else {
        return cancelled();
    };
    let mode = MODES[choice];
    let selected: Vec<_> = selections
        .into_iter()
        .map(|index| targets[index].clone())
        .collect();
    let prepared = executor::prepare(&selected, root, protected_dirs, verbose);
    print_prepared(&prepared, mode);
    println!("{METHOD}: {}", MODE_LABELS[choice]);
    executor::ensure_prepared(&prepared, mode, &[])?;
    if Confirm::with_theme(&theme)
        .with_prompt(CONFIRM_PROMPT)
        .default(false)
        .interact_opt()?
        != Some(true)
    {
        return cancelled();
    }
    let results = executor::execute(&prepared, root, protected_dirs, mode, verbose);
    print_results(&results, &[], mode.delete_branch);
    executor::ensure_complete(&results, &[])
}

/// 依有效工作目錄解析搜尋範圍與目標；空路徑清單表示明確選擇全部候選。
pub fn execute(
    effective_dir: &Path,
    protected_dirs: &[PathBuf],
    root: &Path,
    paths: &[PathBuf],
    mode: DeleteMode,
    dry_run: bool,
    verbose: bool,
) -> Result<()> {
    let requested_root = effective_dir.join(root);
    let discovery = discovery::discover(&requested_root, protected_dirs, verbose)?;
    print_messages(EXCLUDED, &discovery.excluded);
    // 搜尋不完整時不能以找到的子集合開始刪除，即使使用者指定單一路徑。
    executor::ensure_prepared(&[], mode, &discovery.errors)?;
    let targets = if paths.is_empty() {
        discovery.targets
    } else {
        let mut seen = HashSet::new();
        let mut selected = Vec::new();
        for path in paths {
            let actual = discovery::ensure_within_root(&effective_dir.join(path), &requested_root)?;
            let target = discovery
                .targets
                .iter()
                .find(|target| target.path == actual)
                .with_context(|| format!("{UNKNOWN_TARGET}：{path:?}"))?;
            if seen.insert(actual) {
                selected.push(target.clone());
            }
        }
        selected
    };
    let prepared = executor::prepare(&targets, Some(&discovery.root), protected_dirs, verbose);
    if dry_run {
        print_prepared(&prepared, mode);
    }
    executor::ensure_prepared(&prepared, mode, &[])?;
    if dry_run {
        return Ok(());
    }
    let results = executor::execute(
        &prepared,
        Some(&discovery.root),
        protected_dirs,
        mode,
        verbose,
    );
    print_results(&results, &[], mode.delete_branch);
    executor::ensure_complete(&results, &[])
}

/// 互動式跨儲存庫批次刪除 worktree，保護啟動與有效工作目錄。
pub fn run(verbose: bool, protected_dirs: &[PathBuf]) -> Result<()> {
    let effective_dir =
        canonical_path(&std::env::current_dir().context(CURRENT_DIRECTORY_FAILED)?)?;
    let entries = git::list_worktrees_at(&effective_dir, verbose)?;
    let main = entries
        .iter()
        .find(|entry| entry.is_main)
        .context(MAIN_MISSING)?;
    let main_root = canonical_path(Path::new(&main.path))?;
    let default_root = main_root
        .parent()
        .context(ROOT_PARENT_MISSING)?
        .join(ROOT_DIRECTORY);
    let default_text = default_root.to_str().context(INVALID_ROOT_TEXT)?;
    let theme = ColorfulTheme::default();
    let has_controls = default_text.chars().any(char::is_control);
    let root_prompt = if has_controls {
        ROOT_CONTROL_PROMPT
    } else {
        ROOT_PROMPT
    };
    if has_controls {
        println!("{default_root:?}");
    }
    let input: String = Input::with_theme(&theme)
        .with_prompt(root_prompt)
        .with_initial_text(if has_controls { "" } else { default_text })
        .allow_empty(true)
        .interact_text()?;
    let root = if input.is_empty() {
        if has_controls {
            default_root
        } else {
            return cancelled();
        }
    } else {
        effective_dir.join(input)
    };

    println!("{SEARCHING}");
    let discovery = discovery::discover(&root, protected_dirs, verbose)?;
    print_messages(EXCLUDED, &discovery.excluded);
    executor::ensure_prepared(&[], MODES[0], &discovery.errors)?;
    select_and_execute(
        &discovery.targets,
        Some(&discovery.root),
        protected_dirs,
        verbose,
    )
}
