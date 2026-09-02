use anyhow::Result;
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm, FuzzySelect};

use crate::git;

pub fn run(verbose: bool) -> Result<()> {
    let worktrees = git::list_worktrees(verbose)?;

    if worktrees.len() <= 1 {
        println!(
            "{}",
            style("No other worktrees to copy changes from.").yellow()
        );
        return Ok(());
    }

    // Build display items
    let display_items: Vec<String> = worktrees
        .iter()
        .map(|wt| {
            let label = if wt.is_main { "(main)" } else { "" };
            if wt.branch.is_empty() {
                format!("{} (detached HEAD) {}", wt.path, label)
            } else {
                format!("{} [{}] {}", wt.path, wt.branch, label)
            }
        })
        .collect();

    let display_refs: Vec<&str> = display_items.iter().map(|s| s.as_str()).collect();

    let theme = ColorfulTheme::default();
    let selection = FuzzySelect::with_theme(&theme)
        .with_prompt("Select a worktree to copy changes from")
        .items(&display_refs)
        .default(0)
        .interact_opt()?;

    let selection = match selection {
        Some(s) => s,
        None => {
            println!("{}", style("Operation cancelled.").yellow());
            return Ok(());
        }
    };

    let selected = &worktrees[selection];

    println!(
        "Selected source worktree: {} [{}]",
        style(&selected.path).cyan().bold(),
        style(&selected.branch).green()
    );

    // Check if source has changes
    let has_changes = git::worktree_has_changes(&selected.path, verbose)?;
    if !has_changes {
        println!(
            "{}",
            style("The selected worktree has no uncommitted changes to copy.").yellow()
        );
        return Ok(());
    }

    // Get the diff
    let diff = git::get_worktree_diff(&selected.path, verbose)?;
    if diff.is_empty() {
        println!(
            "{}",
            style("No diff found in the selected worktree (changes may be untracked files only).")
                .yellow()
        );
        // Still try to copy untracked files
        let confirmed = Confirm::new()
            .with_prompt("Copy untracked files from the source worktree?")
            .default(true)
            .interact()?;

        if confirmed {
            git::copy_untracked_files(&selected.path, verbose)?;
            println!("{}", style("✓ Untracked files copied.").green());
        }
        return Ok(());
    }

    // Check if the diff can be applied cleanly
    let can_apply = git::apply_diff(&diff, verbose)?;

    if can_apply {
        let confirmed = Confirm::new()
            .with_prompt("Apply changes from the selected worktree to the current worktree?")
            .default(true)
            .interact()?;

        if !confirmed {
            println!("{}", style("Operation cancelled.").yellow());
            return Ok(());
        }

        git::force_apply_diff(&diff, verbose)?;
        println!("{}", style("✓ Changes applied successfully.").green());
    } else {
        println!(
            "\n{}",
            style("⚠ The changes cannot be applied cleanly - there may be conflicts!")
                .red()
                .bold()
        );

        let proceed = Confirm::new()
            .with_prompt("Attempt to apply with 3-way merge (conflicts will be marked)?")
            .default(false)
            .interact()?;

        if !proceed {
            println!("{}", style("Operation cancelled.").yellow());
            return Ok(());
        }

        match git::apply_diff_with_fallback(&diff, verbose) {
            Ok(()) => {
                println!(
                    "{}",
                    style("✓ Changes applied with 3-way merge. Please check for conflict markers.")
                        .green()
                );
            }
            Err(e) => {
                println!(
                    "{}",
                    style(format!("✗ Failed to apply changes: {}", e)).red()
                );
                return Ok(());
            }
        }
    }

    // Copy untracked files too
    let copy_untracked = Confirm::new()
        .with_prompt("Also copy untracked files from the source worktree?")
        .default(true)
        .interact()?;

    if copy_untracked {
        git::copy_untracked_files(&selected.path, verbose)?;
        println!("{}", style("✓ Untracked files copied.").green());
    }

    println!("\n{}", style("Done!").green().bold());
    Ok(())
}
