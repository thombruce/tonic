use clap::{CommandFactory, Parser, Subcommand};

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
        /// Branch to check out in the new worktree (with --detach: any tag, SHA or ref)
        #[arg(required_unless_present = "pr")]
        branch: Option<String>,
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
        /// Create a detached worktree at BRANCH, which may be any tag, SHA or ref
        #[arg(long, conflicts_with_all = ["new_branch", "base", "remote"])]
        detach: bool,
        /// Check out pull request N into its own worktree, via the forge's CLI (gh)
        #[arg(long, value_name = "N", conflicts_with_all = ["branch", "new_branch", "base", "remote", "fetch", "detach"])]
        pr: Option<u32>,
    },
    /// List all worktrees
    #[command(visible_alias = "ls")]
    List {
        /// Show the full status breakdown (staged/unstaged/untracked) and full
        /// branch names in lineage, instead of the compact form
        #[arg(short, long)]
        verbose: bool,
        /// Machine-readable records (key/value lines, one block per worktree)
        #[arg(long, conflicts_with_all = ["json", "verbose"])]
        porcelain: bool,
        /// Machine-readable JSON array of worktrees
        #[arg(long, conflicts_with_all = ["porcelain", "verbose"])]
        json: bool,
    },
    /// Print the path of the worktree for BRANCH (used by shell integration)
    Cd {
        /// Branch whose worktree path to print
        branch: String,
    },
    /// Switch to BRANCH: cd to its worktree if it has one, else check it out in place
    Checkout {
        /// Branch to switch to (one only on a remote is created, tracking it)
        branch: String,
        /// When BRANCH exists on multiple remotes, track this one
        #[arg(long)]
        remote: Option<String>,
    },
    /// Print a shell function for `cd` integration; eval it in your shell rc
    ShellInit {
        /// Shell to emit the wrapper for
        #[arg(value_parser = ["bash", "zsh", "fish"])]
        shell: String,
    },
    /// Print a shell completion script for the given shell
    Completions {
        /// Shell to emit completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Show the current worktree's stack (tip at top, trunk at bottom)
    Stack,
    /// Move one branch up the stack, toward the tip (a child)
    Up,
    /// Move one branch down the stack, toward the trunk (the parent)
    Down,
    /// Jump to the top of the stack (the tip branch)
    Top,
    /// Jump to the bottom of the stack (the branch on the trunk)
    Bottom,
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
        Cmd::Add {
            branch,
            new_branch,
            base,
            remote,
            fetch,
            detach,
            pr,
        } => match (pr, branch) {
            (Some(n), _) => tonic::add_pr(n),
            (None, Some(branch)) => tonic::add(
                &branch,
                new_branch,
                base.as_deref(),
                remote.as_deref(),
                fetch,
                detach,
            ),
            // clap's required_unless_present rules this out
            (None, None) => Err(anyhow::anyhow!("a branch or --pr is required")),
        },
        Cmd::List {
            verbose,
            porcelain,
            json,
        } => {
            let format = if porcelain {
                tonic::ListFormat::Porcelain
            } else if json {
                tonic::ListFormat::Json
            } else {
                tonic::ListFormat::Human
            };
            tonic::list(format, verbose)
        }
        Cmd::Cd { branch } => tonic::cd(&branch),
        Cmd::Checkout { branch, remote } => tonic::checkout(&branch, remote.as_deref()),
        Cmd::ShellInit { shell } => tonic::shell_init(&shell),
        Cmd::Completions { shell } => {
            // clap glue, not domain logic: generate needs the derived command tree.
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "tonic", &mut std::io::stdout());
            Ok(())
        }
        Cmd::Stack => tonic::stack(),
        Cmd::Up => tonic::up(),
        Cmd::Down => tonic::down(),
        Cmd::Top => tonic::top(),
        Cmd::Bottom => tonic::bottom(),
        Cmd::Rm {
            branch,
            force,
            delete_branch,
        } => tonic::rm(&branch, force, delete_branch),
    }
}
