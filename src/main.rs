mod actions;
mod cli;
mod git;

use anyhow::Result;
use clap::Parser;
use console::style;
use dialoguer::Select;

use cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        eprintln!("[DEBUG] Verbose mode enabled");
        eprintln!("[DEBUG] Absolute path mode: {}", cli.absolute_path);
    }

    // Check if current directory is a git repository
    if !git::is_git_repo() {
        eprintln!(
            "{}",
            style("Error: Current directory is not inside a git repository.")
                .red()
                .bold()
        );
        std::process::exit(1);
    }

    if cli.verbose {
        match git::get_repo_root() {
            Ok(root) => eprintln!("[DEBUG] Repository root: {}", root.display()),
            Err(e) => eprintln!("[DEBUG] Could not determine repo root: {}", e),
        }
    }

    // Select action
    let actions = vec!["create", "delete", "move-to", "copy-changes"];
    let selection = Select::new()
        .with_prompt("Select an action")
        .items(&actions)
        .default(0)
        .interact_opt()?;

    let selection = match selection {
        Some(s) => s,
        None => {
            println!("{}", style("Operation cancelled.").yellow());
            return Ok(());
        }
    };

    match actions[selection] {
        "create" => actions::create::run(cli.verbose, cli.absolute_path)?,
        "delete" => actions::delete::run(cli.verbose)?,
        "move-to" => actions::move_to::run(cli.verbose)?,
        "copy-changes" => actions::copy_changes::run(cli.verbose)?,
        _ => unreachable!(),
    }

    Ok(())
}
