use anyhow::{Context, Result, bail};
use console::style;
use dialoguer::{Confirm, FuzzySelect, Input, Select};
use std::path::PathBuf;

use crate::git;

pub fn run(verbose: bool, absolute_path: bool) -> Result<()> {
    // Move to main worktree before processing
    let main_wt_path = git::get_main_worktree_path(verbose)?;
    std::env::set_current_dir(&main_wt_path)
        .with_context(|| format!("Failed to change directory to main worktree: {}", main_wt_path.display()))?;
    if verbose {
        eprintln!("[DEBUG] Changed directory to main worktree: {}", main_wt_path.display());
    }

    // Get all refs (branches and tags)
    let refs = git::get_all_refs(verbose)?;
    if refs.is_empty() {
        println!("{}", style("No branches or tags found.").red());
        return Ok(());
    }

    // Build display items for selection
    let display_items: Vec<&str> = refs.iter().map(|r| r.display.as_str()).collect();

    let selection = FuzzySelect::new()
        .with_prompt("Select a branch or tag to create a worktree from")
        .items(&display_items)
        .default(0)
        .interact_opt()?;

    let selection = match selection {
        Some(s) => s,
        None => {
            println!("{}", style("Operation cancelled.").yellow());
            return Ok(());
        }
    };

    let selected_ref = &refs[selection];
    println!("Selected: {}", style(&selected_ref.display).green().bold());

    // Ask user whether to create worktree directly or create a new branch first
    let create_options = vec![
        "Create worktree directly from the selected ref",
        "Create a new branch from the selected ref, then create worktree",
    ];

    let create_choice = Select::new()
        .with_prompt("How would you like to create the worktree?")
        .items(&create_options)
        .default(0)
        .interact_opt()?;

    let create_choice = match create_choice {
        Some(c) => c,
        None => {
            println!("{}", style("Operation cancelled.").yellow());
            return Ok(());
        }
    };

    // Determine the branch name for the worktree path
    let (branch_name, new_branch_name) = if create_choice == 1 {
        // User wants to create a new branch
        let new_name: String = Input::new()
            .with_prompt("Enter the new branch name")
            .interact_text()?;

        if new_name.trim().is_empty() {
            bail!("Branch name cannot be empty");
        }

        (new_name.clone(), Some(new_name))
    } else {
        // Use the selected ref's name directly
        let name = selected_ref.name.clone();
        // For remote branches, strip the remote prefix for the folder name
        let folder_name = if selected_ref.ref_type == git::GitRefType::RemoteBranch {
            // e.g., "origin/feature-1" -> "feature-1"
            name.split('/').skip(1).collect::<Vec<_>>().join("/")
        } else {
            name.clone()
        };
        (folder_name, None)
    };

    // Compute the worktree path: ../{repoName}-worktree/{branch_name}
    let repo_name = git::get_repo_name()?;
    let repo_root = git::get_repo_root()?;
    let parent_dir = repo_root
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/"));

    // Sanitize branch name for use as a directory name
    let sanitized_branch = sanitize_branch_name(&branch_name);

    let worktree_base = parent_dir.join(format!("{}-worktree", repo_name));
    let worktree_path = worktree_base.join(&sanitized_branch);

    // Determine the path to use (relative or absolute)
    let display_path = if absolute_path {
        worktree_path.clone()
    } else {
        // Compute relative path from repo root
        pathdiff_relative(&repo_root, &worktree_path)
    };

    println!(
        "Worktree will be created at: {}",
        style(display_path.display()).cyan().bold()
    );

    // Actually create the worktree (always use absolute path for git commands)
    let actual_path = if absolute_path {
        worktree_path.clone()
    } else {
        display_path.clone()
    };

    if let Some(ref new_branch) = new_branch_name {
        git::create_worktree_new_branch(&actual_path, new_branch, &selected_ref.name, verbose)?;
        println!(
            "{}",
            style(format!(
                "Created worktree with new branch '{}' from '{}'",
                new_branch, selected_ref.name
            ))
            .green()
        );
    } else {
        git::create_worktree(&actual_path, &selected_ref.name, verbose)?;
        println!(
            "{}",
            style(format!("Created worktree for '{}'", selected_ref.name)).green()
        );
    }

    // Ask if user wants to move to the new worktree
    let move_to = Confirm::new()
        .with_prompt("Do you want to move to the new worktree?")
        .default(true)
        .interact()?;

    if move_to {
        let target = std::fs::canonicalize(&worktree_path).unwrap_or(worktree_path.clone());
        let target_str = target.display().to_string();
        let target_str = git::strip_unc_prefix(&target_str);
        println!(
            "{}",
            style(format!(
                "To move to the new worktree, run:\n  cd {}",
                target_str
            ))
            .cyan()
        );
        // Print the path so shell wrapper scripts can use it
        println!("COPSE_CD:{}", target_str);
    }

    Ok(())
}

/// Compute a relative path from `base` to `target`.
fn pathdiff_relative(base: &std::path::Path, target: &std::path::Path) -> PathBuf {
    // Try to compute a relative path; strip UNC prefix that canonicalize
    // may produce on Windows so the component comparison works correctly.
    let base = std::fs::canonicalize(base).unwrap_or_else(|_| base.to_path_buf());
    let base = git::strip_unc_prefix_path(base);
    let target_abs = if target.is_absolute() {
        target.to_path_buf()
    } else {
        base.join(target)
    };

    // Simple relative path computation
    let base_components: Vec<_> = base.components().collect();
    let target_components: Vec<_> = target_abs.components().collect();

    // Find common prefix length
    let common_len = base_components
        .iter()
        .zip(target_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut result = PathBuf::new();

    // Add ".." for each remaining base component
    for _ in common_len..base_components.len() {
        result.push("..");
    }

    // Add remaining target components
    for component in &target_components[common_len..] {
        result.push(component);
    }

    if result.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        result
    }
}

/// Sanitize a branch name for use as a directory name.
/// Replaces characters that are problematic on various filesystems.
fn sanitize_branch_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}
