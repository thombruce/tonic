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
        /// Create BRANCH as a new branch
        #[arg(short = 'b', long = "new-branch")]
        new_branch: bool,
    },
    /// List all worktrees
    List,
    /// Remove the worktree checked out for BRANCH
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
        Cmd::Add { branch, new_branch } => tonic::add(&branch, new_branch),
        Cmd::List => tonic::list(),
        Cmd::Rm { branch, force, delete_branch } => tonic::rm(&branch, force, delete_branch),
    }
}
