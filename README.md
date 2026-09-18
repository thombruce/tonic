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

- **Lateral:** jump between **worktrees** (sibling checkouts) — `add`, `list`,
  `cd`, `rm`, with shell integration that drops you into the right directory.
- **Vertical:** navigate **stacked branches** (a series where each builds on the
  one below) — `list` surfaces the inferred stack lineage, and `up`/`down`/
  `top`/`bottom` move along it, keeping local and host-agnostic what `gh stack` /
  `glab stack` do per-provider, without ever rebasing for you.

Both axes are inferred from the commit graph and your worktree layout — no stored
state, no lock-in. A vertical step even unifies the axes: it `cd`s to the target
branch's worktree if it has one, or checks it out in place if it doesn't.

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
tonic list [-v]                # list worktrees (alias: ls; marks current ▸, status, stack lineage; -v: full breakdown)
tonic cd  <branch>             # print a worktree's path (cd's into it via shell integration)
tonic up / down                # move one branch up (toward the tip) / down (toward the trunk) the stack
tonic top / bottom             # jump to the tip / base of the stack
tonic rm  <branch> [-f] [-d]   # remove a worktree (alias: remove; -f: force, -d: also delete branch)
tonic shell-init <shell>       # print the shell function for cd integration
```

`add` resolves the worktree path from config, runs `git worktree add`, transfers
`.worktreeinclude` entries (copy or symlink per pattern), then runs `post_create`
hooks. `rm` runs `pre_remove` hooks, removes the worktree, and optionally deletes
the branch.

Worktrees are **sibling directories**, so `list` stays flat, and paths are shown
relative to the directory they live in (not as long absolute paths). When a
branch is in a stack, its row is annotated with the inferred lineage,
root-anchored, with the row's **own branch shown as `*`** (a footnote back-ref to
its label, so the name isn't repeated) — e.g. `main → baz → *` on `birthday`'s
row, or `main → * → birthday` on `baz`'s. A worktree with children stacked on it
continues the chain — ` → child` for a single child, ` → [N]` for a fork of N.
Derived from the commit graph (no stored state); a freshly-created branch with no
commits of its own shows no lineage yet and gains it once it has a commit. The
lineage can name a stacked ancestor even when that ancestor has no worktree of
its own. The current worktree is marked `▸`.

Each row ends with a compact status, shown only when it applies, following the
usual git-prompt conventions:

- `!N` — N changed files (a file with both staged and unstaged edits counts
  once); shown in yellow.
- `↑N` / `↓N` — commits ahead of / behind the branch's upstream, in cyan
  (omitted when there's no upstream).

`tonic list -v` (`--verbose`) expands both: the dirty count splits into
`+staged *unstaged ?untracked`, and the lineage prints full branch names instead
of the `*` self-marker — the lossless "full picture".

### Stack navigation

`cd` moves **laterally** between worktrees; `up`/`down`/`top`/`bottom` move
**vertically** along a stack of dependent branches, using the same inferred
lineage `list` shows (no stored state). A stack is a column: `up`/`top` climb
toward the tip (a child, newer work), `down`/`bottom` descend toward the trunk
(the parent).

A step goes to the target branch whichever way applies — if the branch **has a
worktree**, tonic `cd`s there (needs [shell integration](#shell-integration-optional));
if it **doesn't**, tonic `git checkout`s it **in place** in the current worktree.
So a stack can live across many worktrees, in one, or a mix, and navigation just
works. This is structure and movement only — no rebase or re-parenting; use
`git rebase` (or `gh stack` / `glab`) to restructure.

A fork (a branch with several children) has no single target: `up`/`top` error
and name the children so you can `tonic cd`/`tonic add` the one you want. An
in-place checkout is refused when the worktree has uncommitted changes (commit or
stash first) — so a vertical step never silently moves your work onto another
branch. A lateral step (to a branch that has its own worktree) never touches your
files.

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

# The repo's trunk branch. Used as the base for `list` stack lineage and as the
# preferred main worktree for bare-repo fallbacks. tonic auto-detects "main"
# then "master"; set this for other conventions (e.g. "develop"). A value that
# doesn't match an existing branch is ignored (falls back to main/master).
default_branch = "develop"

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
