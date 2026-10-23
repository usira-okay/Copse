mod actions;
mod app;
mod cli;
mod cli_command;
mod git;
mod interactive;
mod list_entry;
mod repository;
mod worktree_selection;

#[cfg(test)]
mod test_support;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    app::run(cli::Cli::parse())
}
