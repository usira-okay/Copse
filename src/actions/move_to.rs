use anyhow::Result;
use console::style;
use dialoguer::Select;

use crate::git;

pub fn run(verbose: bool) -> Result<()> {
    let worktrees = git::list_worktrees(verbose)?;

    if worktrees.len() <= 1 {
        println!("{}", style("No other worktrees to move to.").yellow());
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

    let selection = Select::new()
        .with_prompt("Select a worktree to move to")
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

    let target = std::fs::canonicalize(&selected.path)
        .unwrap_or_else(|_| std::path::PathBuf::from(&selected.path));

    // Strip UNC extended-length path prefix for Windows compatibility.
    let target_str = target.display().to_string();
    let target_str = git::strip_unc_prefix(&target_str);

    println!(
        "{}",
        style(format!("To move to the worktree, run:\n  cd {}", target_str)).cyan()
    );
    // Print the path so shell wrapper scripts can use it
    println!("COPSE_CD:{}", target_str);

    Ok(())
}
