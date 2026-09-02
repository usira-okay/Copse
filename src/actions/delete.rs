use anyhow::{Context, Result};
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm, FuzzySelect, MultiSelect};

use crate::git;

pub fn run(verbose: bool) -> Result<()> {
    // Capture the original working directory before switching to main worktree
    let original_dir = std::env::current_dir().context("Failed to get current directory")?;
    let original_dir = match std::fs::canonicalize(&original_dir) {
        Ok(p) => p,
        Err(e) => {
            if verbose {
                eprintln!(
                    "[DEBUG] Could not canonicalize original dir {:?}: {}",
                    original_dir, e
                );
            }
            original_dir
        }
    };
    let original_dir = git::strip_unc_prefix_path(original_dir);

    if verbose {
        eprintln!(
            "[DEBUG] Original working directory: {}",
            original_dir.display()
        );
    }

    let worktrees = git::list_worktrees(verbose)?;

    // Filter out the main worktree and the worktree matching the original
    // working directory — neither should be deletable.
    let removable: Vec<_> = worktrees
        .iter()
        .filter(|wt| {
            if wt.is_main {
                return false;
            }
            // Check if this worktree matches the original working directory
            let wt_path = match std::fs::canonicalize(&wt.path) {
                Ok(p) => p,
                Err(e) => {
                    if verbose {
                        eprintln!(
                            "[DEBUG] Could not canonicalize worktree path {}: {}",
                            wt.path, e
                        );
                    }
                    std::path::PathBuf::from(&wt.path)
                }
            };
            let wt_path = git::strip_unc_prefix_path(wt_path);
            if wt_path == original_dir || original_dir.starts_with(&wt_path) {
                if verbose {
                    eprintln!(
                        "[DEBUG] Skipping current worktree from deletion list: {}",
                        wt.path
                    );
                }
                return false;
            }
            true
        })
        .collect();

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
    let theme = ColorfulTheme::default();

    // Allow multi-select
    let selections = MultiSelect::with_theme(&theme)
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
        "[x]  Delete worktree(s) but keep branch(es)",
        "⚡  Force delete worktree(s) but keep branch(es)",
        "[x]🌿 Delete worktree(s) and delete branch(es)",
        "⚡🌿 Force delete worktree(s) and delete branch(es)",
    ];

    let delete_choice = FuzzySelect::with_theme(&theme)
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

    // Build a detailed confirmation prompt
    let branch_list: Vec<String> = selected_worktrees
        .iter()
        .map(|wt| {
            if wt.branch.is_empty() {
                format!("  {} (detached HEAD)", wt.path)
            } else {
                format!("  {} [{}]", wt.path, wt.branch)
            }
        })
        .collect();
    let method_desc = &delete_options[delete_choice];
    println!(
        "\n{}\n{}\n{} {}",
        style("Worktrees to delete:").bold(),
        branch_list.join("\n"),
        style("Method:").bold(),
        method_desc,
    );

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
