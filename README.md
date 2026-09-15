# tonic

A git worktree companion — `git` and `tonic`.

A small CLI that makes git worktrees pleasant: it respects `.worktreeinclude`
(copying or symlinking your gitignored files into new worktrees), lets you
configure where worktrees are created, and runs lifecycle hooks on create and
remove — so per-worktree dev databases, containers, or build dirs can spin up
and tear down cleanly.

## Philosophy

tonic is about **moving around your git work locally** — provider-agnostic, no
rebasing — along two axes:

- **Lateral:** jump between **worktrees** (sibling checkouts). This is tonic
  today — `add`, `list`, `cd`, `rm`, with shell integration that drops you into
  the right directory.
- **Vertical:** navigate **stacked branches** (a series where each builds on the
  one below). This is where tonic is heading — surfacing stack lineage and
  moving up and down it, keeping local and host-agnostic what `gh stack` /
  `glab stack` do per-provider, without ever rebasing for you.

## Installation

```
brew install thombruce/tap/tonic   # macOS / Linux (Homebrew)
cargo install git-tonic            # from crates.io
```

The crate is published as `git-tonic` (the name `tonic` was taken on crates.io),
but the installed binary is `tonic`.

### Shell integration (optional)

`tonic` can drop you straight into a worktree after `add`, `tonic cd <branch>`
jumps to an existing one, and removing the worktree you're standing in moves you
back to the main worktree instead of stranding you in a deleted directory. A
binary can't change its parent shell's directory, so this is a small shell
function you source once. Add to your shell's rc:

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
tonic add <branch> [-b] [--base <ref>] [--remote <name>] [--fetch]   # create a worktree
tonic list                     # list worktrees (alias: ls; marks current with *, flags dirty ones)
tonic cd  <branch>             # print a worktree's path (cd's into it via shell integration)
tonic rm  <branch> [-f] [-d]   # remove a worktree (alias: remove; -f: force, -d: also delete branch)
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
   Matches the remote-tracking ref if it exists locally; pass `--fetch` (or set
   `fetch = true`) to fetch first so a branch not yet fetched is picked up.
3. **Nowhere** → create a new branch from `HEAD` (the current worktree's HEAD, so
   a branch created from another worktree stacks on it). Set `base` in config to
   root new branches at a fixed ref (e.g. `main`) instead.

Flags override the default: `-b` forces a new branch (from `HEAD`, skipping the
remote-tracking step), `--base <ref>` starts a new branch from `<ref>` (overrides
the `base` config). `base`/`--base` only apply when *creating* a new branch — they
have no effect when checking out an existing local or remote branch.

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

# Default start-point (any ref) for a NEW branch, when --base isn't given.
# Without this, new branches are created from the current worktree's HEAD.
# No effect when checking out an existing local or remote branch.
base = "main"

# Fetch before resolving a non-local branch, so a branch that's on a remote but
# not yet fetched is picked up (like passing --fetch every time). Only fetches
# when the branch isn't already local; non-fatal if offline. Prefer the --fetch
# flag if you only want it occasionally.
fetch = true

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
