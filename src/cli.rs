use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "copse",
    version,
    about = "A CLI tool to quickly control git worktree"
)]
pub struct Cli {
    /// Show verbose output for debugging
    #[arg(short, long)]
    pub verbose: bool,

    /// Use absolute paths when creating worktrees (default: relative paths)
    #[arg(short = 'a', long = "absolute-path")]
    pub absolute_path: bool,
}
