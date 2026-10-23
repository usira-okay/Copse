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
        None => original_dir.clone(),
    };
    let protected_dirs = [original_dir, directory.clone()];
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
        Some(CliCommand::Delete {
            paths,
            branches,
            force,
            delete_branch,
            ..
        }) => actions::delete::execute(
            &directory,
            &protected_dirs,
            &paths,
            &branches,
            actions::batch_delete::DeleteMode {
                force,
                delete_branch,
            },
            cli.verbose,
        ),
        Some(CliCommand::BatchDelete {
            root,
            paths,
            dry_run,
            force,
            delete_branch,
            ..
        }) => actions::batch_delete::execute(
            &directory,
            &protected_dirs,
            &root,
            &paths,
            actions::batch_delete::DeleteMode {
                force,
                delete_branch,
            },
            dry_run,
            cli.verbose,
        ),
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
        Some(CliCommand::CopyChanges {
            from_path,
            from_branch,
            include_untracked,
            three_way,
        }) => {
            let entries = git::list_worktrees_at(&directory, cli.verbose)?;
            let source = worktree_selection::resolve(
                &entries,
                &directory,
                from_path.as_deref(),
                from_branch.as_deref(),
            )?;
            actions::copy_changes::execute(
                &directory,
                source,
                include_untracked,
                three_way,
                cli.verbose,
            )
        }
        None => {
            std::env::set_current_dir(&directory).context(CHANGE_DIRECTORY_FAILED)?;
            interactive::run(cli.verbose, cli.absolute_path, &protected_dirs)
        }
    }
}
