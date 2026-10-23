use crate::{actions, cli::Cli, cli_command::CliCommand, git, interactive, worktree_selection};
use anyhow::{Context, Result};

const CURRENT_DIRECTORY_FAILED: &str = "無法取得啟動目錄";
const CHANGE_DIRECTORY_FAILED: &str = "無法使用指定的工作目錄";

/// 組裝工作目錄與命令，所有互動提示由互動入口處理。
pub fn run(cli: Cli) -> Result<()> {
    git::require_batch_delete_git(cli.verbose)?;
    let original_dir = worktree_selection::canonical_path(
        &std::env::current_dir().context(CURRENT_DIRECTORY_FAILED)?,
    )?;
    let directory = match cli.directory {
        Some(path) => worktree_selection::canonical_path(&original_dir.join(path))?,
        None => original_dir,
    };
    match cli.command {
        Some(CliCommand::Create {
            reference,
            new_branch,
        }) => actions::create::execute(
            &directory,
            &reference,
            new_branch.as_deref(),
            cli.verbose,
            cli.absolute_path,
        ),
        Some(CliCommand::List { json }) => actions::list::run(&directory, json, cli.verbose),
        Some(CliCommand::MoveTo { path, branch }) => {
            let entries = git::list_worktrees_at(&directory, cli.verbose)?;
            let selected = worktree_selection::resolve(
                &entries,
                &directory,
                path.as_deref(),
                branch.as_deref(),
            )?;
            actions::move_to::execute(selected)
        }
        None => {
            std::env::set_current_dir(&directory).context(CHANGE_DIRECTORY_FAILED)?;
            interactive::run(cli.verbose, cli.absolute_path)
        }
    }
}
