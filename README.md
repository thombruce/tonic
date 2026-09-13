# tonic

A git worktree companion — `git` and `tonic`.

A small CLI that makes git worktrees pleasant: it respects `.worktreeinclude`
(copying or symlinking your gitignored files into new worktrees), lets you
configure where worktrees are created, and runs lifecycle hooks on create and
remove — so per-worktree dev databases, containers, or build dirs can spin up
and tear down cleanly.

## Installation

```
brew install thombruce/tap/tonic   # macOS / Linux (Homebrew)
cargo install git-tonic            # from crates.io
```

The crate is published as `git-tonic` (the name `tonic` was taken on crates.io),
but the installed binary is `tonic`.

### Shell integration (optional)

`tonic` can drop you straight into a worktree after `add`, and `tonic cd <branch>`
jumps to an existing one. A binary can't change its parent shell's directory, so
this is a small shell function you source once. Add to your shell's rc:

```sh
# ~/.bashrc or ~/.zshrc
eval "$(tonic shell-init zsh)"      # or: bash

# ~/.config/fish/config.fish
tonic shell-init fish | source
```

Without it `tonic` still works — `tonic add` just prints the new worktree path
(so `cd "$(tonic add foo)"` works too).

## Commands

```
tonic add <branch> [-b] [--base <ref>] [--remote <name>]   # create a worktree
tonic list                     # list worktrees (marks current with *, flags dirty ones)
tonic cd  <branch>             # print a worktree's path (cd's into it via shell integration)
tonic rm  <branch> [-f] [-d]   # remove a worktree (-f: force, -d: also delete branch)
tonic shell-init <shell>       # print the shell function for cd integration
```

`add` resolves the worktree path from config, runs `git worktree add`, transfers
`.worktreeinclude` entries (copy or symlink per pattern), then runs `post_create`
hooks. `rm` runs `pre_remove` hooks, removes the worktree, and optionally deletes
the branch.

### How `tonic add <branch>` resolves the branch

1. **Local branch exists** → check it out. (If it's already checked out in
   another worktree, tonic errors and points you at `tonic cd`.)
2. **No local branch, but a remote has it** → create a local branch tracking the
   remote (`origin` preferred; `--remote <name>` to pick among several).
   Requires the remote-tracking ref to exist locally — `git fetch` first if not.
3. **Nowhere** → create a new branch from `HEAD`.

Flags override the default: `-b` forces a new branch (from `HEAD`, skipping the
remote-tracking step), `--base <ref>` starts a new branch from `<ref>`.

## Configuration

Config is TOML, merged from a chain (later wins per key, mirroring git's
system < global < local precedence):

1. `~/.config/tonic/config.toml` — global defaults
2. `<repo>/tonic.toml` — shared, committed
3. `<gitdir>/tonic.toml` — per-repo, private (untracked, lives inside `.git/`)

Merging is **per top-level key, replace not append**: if a higher-priority file
sets `[[include]]` or `[[hooks]]`, it replaces that whole list — it does not add
to the lists from lower-priority files. To extend global hooks or includes in a
more specific file, restate the full list there.

```toml
# Path template for new worktrees. Placeholders: {repo}, {branch}.
# Resolved relative to the repo's parent dir (normal repo) or the bare dir
# itself (bare repo). Default: "{repo}-{branch}" normal (a sibling of the
# working tree), "{branch}" bare. A "/" in a branch name is flattened to "-"
# in the path. Example override — nest worktrees inside the repo instead:
worktree_path = "{repo}/.worktrees/{branch}"

# Per-pattern transfer mode for entries listed in .worktreeinclude.
# Default mode is "copy".
[[include]]
pattern = "node_modules"
mode = "symlink"

[[include]]
pattern = ".env"
mode = "copy"

# Lifecycle hooks. Placeholders: {repo}, {branch}, {worktree_path}.
# Events: post_create, pre_remove. Run via `sh -c` in the worktree dir.
[[hooks]]
event = "post_create"
run = "createdb tonic_{branch}"

[[hooks]]
event = "pre_remove"
run = "dropdb tonic_{branch}"
```

### `.worktreeinclude`

One glob per line (relative to the repo root; `#` comments allowed). These are
the gitignored files/dirs copied or symlinked into each new worktree:

```
.env
node_modules
```

Only files that are **both** matched here **and** gitignored are transferred —
tracked files are never copied, so you can't accidentally fork a committed file.

Entries are sourced from the working tree you invoke `tonic` in. In a bare repo
invoked from the bare dir (no working tree), tonic sources from the `main/` then
`master/` worktree.

## Bare repositories

`tonic` works from inside a bare repo (e.g. a `git clone --bare`). New worktrees
default to siblings inside the bare dir — `barerepo.git/main`,
`barerepo.git/feature` — and, when invoked from the bare dir, `.worktreeinclude`
is sourced from the `main/` (then `master/`) worktree.

## Not yet supported

- Full gitignore glob semantics (patterns are top-level/relative, not recursive)
- Windows symlinks (symlink mode is unix-only)
