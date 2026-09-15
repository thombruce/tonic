use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tonic", version, about = "A git worktree companion")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a worktree for BRANCH
    Add {
        /// Branch to check out in the new worktree
        branch: String,
        /// Create BRANCH as a new branch (from HEAD unless --base)
        #[arg(short = 'b', long = "new-branch")]
        new_branch: bool,
        /// Create a new branch from this ref instead of HEAD (implies --new-branch)
        #[arg(long)]
        base: Option<String>,
        /// When BRANCH exists on multiple remotes, track this one
        #[arg(long)]
        remote: Option<String>,
        /// Fetch before resolving, so a branch that's on a remote but not yet
        /// fetched is picked up
        #[arg(long)]
        fetch: bool,
    },
    /// List all worktrees
    #[command(visible_alias = "ls")]
    List,
    /// Print the path of the worktree for BRANCH (used by shell integration)
    Cd {
        /// Branch whose worktree path to print
        branch: String,
    },
    /// Print a shell function for `cd` integration; eval it in your shell rc
    ShellInit {
        /// Shell to emit the wrapper for
        #[arg(value_parser = ["bash", "zsh", "fish"])]
        shell: String,
    },
    /// Remove the worktree checked out for BRANCH
    #[command(visible_alias = "remove")]
    Rm {
        /// Branch whose worktree should be removed
        branch: String,
        /// Force removal of a dirty/locked worktree (and unmerged branch)
        #[arg(short = 'f', long)]
        force: bool,
        /// Also delete the branch after removing its worktree
        #[arg(short = 'd', long = "delete-branch")]
        delete_branch: bool,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Add { branch, new_branch, base, remote, fetch } => {
            tonic::add(&branch, new_branch, base.as_deref(), remote.as_deref(), fetch)
        }
        Cmd::List => tonic::list(),
        Cmd::Cd { branch } => tonic::cd(&branch),
        Cmd::ShellInit { shell } => tonic::shell_init(&shell),
        Cmd::Rm { branch, force, delete_branch } => tonic::rm(&branch, force, delete_branch),
    }
}
