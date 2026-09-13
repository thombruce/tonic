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

## Commands

```
tonic add <branch> [-b]        # create a worktree for <branch> (-b: new branch)
tonic list                     # list worktrees
tonic rm  <branch> [-f] [-d]   # remove a worktree (-f: force, -d: also delete branch)
```

`add` resolves the worktree path from config, runs `git worktree add`, transfers
`.worktreeinclude` entries (copy or symlink per pattern), then runs `post_create`
hooks. `rm` runs `pre_remove` hooks, removes the worktree, and optionally deletes
the branch.

## Configuration

Config is TOML, merged from a chain (later wins per key, mirroring git's
system < global < local precedence):

1. `~/.config/tonic/config.toml` — global defaults
2. `<repo>/tonic.toml` — shared, committed
3. `<gitdir>/tonic.toml` — per-repo, private (untracked, lives inside `.git/`)

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
