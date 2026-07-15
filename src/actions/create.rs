use anyhow::{Context, Result, bail};
use console::style;
use dialoguer::{FuzzySelect, Input, Select};
use std::path::{Path, PathBuf};

use crate::git;

pub fn run(verbose: bool, absolute_path: bool) -> Result<()> {
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
        "🌿  Create a new branch from the selected ref, then create worktree",
        "📂  Create worktree directly from the selected ref",
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
    let (branch_name, new_branch_name) = if create_choice == 0 {
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

    // Compute the worktree path: ../worktree/{repoName}/{branch_name}, anchored
    // to the main worktree so the layout is consistent no matter which
    // worktree `create` is run from.
    let main_root = git::get_main_worktree_root(verbose)?;
    let repo_name = main_root
        .file_name()
        .context("Could not determine repository name")?
        .to_string_lossy()
        .to_string();
    let worktree_path = build_worktree_path(&main_root, &repo_name, &branch_name);

    // Determine the path to use (relative or absolute). The relative display
    // is computed from the current worktree's root (via `get_repo_root`, not
    // `main_root`), so the printed `cd` hint is directly usable from wherever
    // the user is standing (this matches the pre-existing display behavior).
    let current_worktree_root = git::get_repo_root()?;
    let display_path = if absolute_path {
        worktree_path.clone()
    } else {
        pathdiff_relative(&current_worktree_root, &worktree_path)
    };

    println!(
        "Worktree will be created at: {}",
        style(display_path.display()).cyan().bold()
    );

    // Always pass the absolute path to git commands, regardless of the display preference.
    if let Some(ref new_branch) = new_branch_name {
        git::create_worktree_new_branch(&worktree_path, new_branch, &selected_ref.name, verbose)?;
        println!(
            "{}",
            style(format!(
                "Created worktree with new branch '{}' from '{}'",
                new_branch, selected_ref.name
            ))
            .green()
        );
    } else {
        git::create_worktree(&worktree_path, &selected_ref.name, verbose)?;
        println!(
            "{}",
            style(format!("Created worktree for '{}'", selected_ref.name)).green()
        );
    }

    // Automatically move to the new worktree
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

fn build_worktree_path(repo_root: &Path, repo_name: &str, branch_name: &str) -> PathBuf {
    let parent_dir = repo_root
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/"));
    // Canonicalize parent_dir so path components are consistent with the
    // canonicalized base used inside pathdiff_relative.
    let parent_dir = std::fs::canonicalize(&parent_dir)
        .map(git::strip_unc_prefix_path)
        .unwrap_or(parent_dir);

    parent_dir
        .join("worktree")
        .join(repo_name)
        .join(sanitize_branch_name(branch_name))
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

#[cfg(test)]
mod tests {
    use super::{build_worktree_path, pathdiff_relative};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_temp_repo_root(repo_name: &str) -> (PathBuf, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!("copse-test-{unique}"));
        let repo_root = temp_root.join(repo_name);
        std::fs::create_dir_all(&repo_root).unwrap();
        (temp_root, repo_root)
    }

    #[test]
    fn builds_worktree_path_under_worktree_directory() {
        let (temp_root, repo_root) = make_temp_repo_root("ProjectA");

        let path = build_worktree_path(&repo_root, "ProjectA", "feature-1");

        assert_eq!(
            path,
            temp_root.join("worktree").join("ProjectA").join("feature-1")
        );

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn sanitizes_branch_name_in_worktree_path() {
        let (temp_root, repo_root) = make_temp_repo_root("ProjectA");

        let path = build_worktree_path(&repo_root, "ProjectA", "feature/test:1");

        assert_eq!(
            path,
            temp_root
                .join("worktree")
                .join("ProjectA")
                .join("feature_test_1")
        );

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn computes_relative_path_for_nested_worktree_directory() {
        let (temp_root, repo_root) = make_temp_repo_root("ProjectA");
        let worktree_path = temp_root.join("worktree").join("ProjectA").join("feature-1");

        let relative = pathdiff_relative(&repo_root, &worktree_path);

        assert_eq!(relative, PathBuf::from("../worktree/ProjectA/feature-1"));

        std::fs::remove_dir_all(temp_root).unwrap();
    }
}
