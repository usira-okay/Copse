use anyhow::{Context, Result};
use console::style;
use dialoguer::{Confirm, MultiSelect, Select};

use crate::git;

pub fn run(verbose: bool) -> Result<()> {
    // Move to main worktree before processing
    let main_wt_path = git::get_main_worktree_path(verbose)?;
    std::env::set_current_dir(&main_wt_path)
        .with_context(|| format!("Failed to change directory to main worktree: {}", main_wt_path.display()))?;
    if verbose {
        eprintln!("[DEBUG] Changed directory to main worktree: {}", main_wt_path.display());
    }

    let worktrees = git::list_worktrees(verbose)?;

    // Filter out the main worktree - it cannot be removed
    let removable: Vec<_> = worktrees.iter().filter(|wt| !wt.is_main).collect();

    if removable.is_empty() {
        println!(
            "{}",
            style("No additional worktrees found to delete.").yellow()
        );
        return Ok(());
    }

    // Build display items
    let display_items: Vec<String> = removable
        .iter()
        .map(|wt| {
            if wt.branch.is_empty() {
                format!("{} (detached HEAD)", wt.path)
            } else {
                format!("{} [{}]", wt.path, wt.branch)
            }
        })
        .collect();

    let display_refs: Vec<&str> = display_items.iter().map(|s| s.as_str()).collect();

    // Allow multi-select
    let selections = MultiSelect::new()
        .with_prompt("Select worktrees to delete (use Space to select, Enter to confirm)")
        .items(&display_refs)
        .interact_opt()?;

    let selections = match selections {
        Some(s) if !s.is_empty() => s,
        _ => {
            println!(
                "{}",
                style("No worktrees selected. Operation cancelled.").yellow()
            );
            return Ok(());
        }
    };

    let selected_worktrees: Vec<_> = selections.iter().map(|&i| removable[i].clone()).collect();

    println!("\n{}", style("Selected worktrees:").bold());
    for wt in &selected_worktrees {
        println!(
            "  {} [{}]",
            style(&wt.path).cyan(),
            style(&wt.branch).green()
        );
    }

    // Choose delete option
    let delete_options = vec![
        "Delete worktree(s) but keep branch(es)",
        "Force delete worktree(s) but keep branch(es)",
        "Delete worktree(s) and delete branch(es)",
        "Force delete worktree(s) and delete branch(es)",
    ];

    let delete_choice = Select::new()
        .with_prompt("\nHow would you like to delete?")
        .items(&delete_options)
        .default(0)
        .interact_opt()?;

    let delete_choice = match delete_choice {
        Some(c) => c,
        None => {
            println!("{}", style("Operation cancelled.").yellow());
            return Ok(());
        }
    };

    let force = delete_choice == 1 || delete_choice == 3;
    let delete_branch = delete_choice == 2 || delete_choice == 3;

    // Show appropriate warning and get confirmation
    if force {
        println!(
            "\n{}",
            style("⚠ Force delete will remove worktree(s) even if they have uncommitted changes!")
                .red()
                .bold()
        );
    } else {
        // Check for uncommitted changes in selected worktrees
        let mut has_changes = false;
        for wt in &selected_worktrees {
            match git::worktree_has_changes(&wt.path, verbose) {
                Ok(true) => {
                    println!(
                        "\n{}",
                        style(format!("⚠ Worktree '{}' has uncommitted changes!", wt.path))
                            .red()
                            .bold()
                    );
                    has_changes = true;
                }
                Ok(false) => {}
                Err(e) => {
                    if verbose {
                        eprintln!("[DEBUG] Could not check changes for {}: {}", wt.path, e);
                    }
                }
            }
        }

        if has_changes {
            let discard = Confirm::new()
                .with_prompt("Some worktrees have uncommitted changes. Discard these changes?")
                .default(false)
                .interact()?;

            if !discard {
                println!("{}", style("Operation cancelled.").yellow());
                return Ok(());
            }
        }
    }

    if delete_branch {
        println!(
            "\n{}",
            style("This will also delete the associated branch(es).").yellow()
        );
    }

    let confirmed = Confirm::new()
        .with_prompt("Are you sure you want to proceed?")
        .default(false)
        .interact()?;

    if !confirmed {
        println!("{}", style("Operation cancelled.").yellow());
        return Ok(());
    }

    // Perform deletion
    for wt in &selected_worktrees {
        // Remove worktree
        match git::remove_worktree(&wt.path, force, verbose) {
            Ok(()) => {
                println!(
                    "{}",
                    style(format!("✓ Removed worktree: {}", wt.path)).green()
                );
            }
            Err(e) => {
                println!(
                    "{}",
                    style(format!("✗ Failed to remove worktree '{}': {}", wt.path, e)).red()
                );
                continue;
            }
        }

        // Delete branch if requested
        if delete_branch && !wt.branch.is_empty() {
            match git::delete_branch(&wt.branch, force, verbose) {
                Ok(()) => {
                    println!(
                        "{}",
                        style(format!("✓ Deleted branch: {}", wt.branch)).green()
                    );
                }
                Err(e) => {
                    println!(
                        "{}",
                        style(format!("✗ Failed to delete branch '{}': {}", wt.branch, e)).red()
                    );
                }
            }
        }
    }

    println!("\n{}", style("Done!").green().bold());
    Ok(())
}
