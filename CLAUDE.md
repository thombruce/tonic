# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

`tonic` is a Rust CLI companion to `git` (the name is the pun) that makes git worktrees pleasant: it respects `.worktreeinclude`, configures where worktrees land, and runs lifecycle hooks on create/remove.

## Commands

```
cargo build              # debug build -> target/debug/tonic
cargo test               # run unit tests
cargo test <name>        # run a single test, e.g. cargo test worktree_base
cargo run -- add -b foo  # run the CLI (args after --)
```

CI (`.github/workflows/ci.yml`) runs `cargo test` and `cargo clippy -- -D warnings` on every push/PR to `main` — clippy warnings fail the build, so run `cargo clippy -- -D warnings` before pushing.

`[lints.clippy]` in `Cargo.toml` **denies panics in production**: `unwrap_used`, `expect_used`, `indexing_slicing`, `arithmetic_side_effects`, `panic`, `todo`, `as_conversions`, etc. Return `Result`/handle the case instead. The `tests` module opts out via `#[allow(...)]` — unwrap/index freely there.

## Workflow

Land changes via a short-lived branch and a PR to `main` — do not commit code changes straight to `main`. Let CI (test + clippy) go green on the PR, then squash-merge. Release version bumps and tagging (see Packaging & release) are the exception: they're the maintainer's release step and go directly on `main`.

## Architecture

Two source files. `src/main.rs` is a thin `clap` shell: each subcommand is a `Cmd` variant that dispatches to one `pub fn` in `src/lib.rs`. All logic lives in `lib.rs`. Adding a command = add a variant + a dispatch line in `main.rs`, and a `pub fn` in `lib.rs`.

**Shells out to `git`; does not link libgit2/gix.** Everything goes through two helpers: `git_run` (runs, bails on nonzero exit) and `git_capture` (returns trimmed stdout). The one exception is `is_ignored`, which calls `git check-ignore --quiet` and inspects the raw exit status directly — exit 1 ("not ignored") is a normal result there, not an error, so it must not use `git_run`.

**`Repo::discover` is the model for normal vs bare repos.** Identity is derived from the *repository*, not the invoking working tree, so it's stable no matter which worktree tonic runs in:
- `root: Option<PathBuf>` is the invoking working tree (`None` when invoked from a bare dir). Used only for `Repo::cwd()` (where git runs) and as the `.worktreeinclude` source — **not** for identity.
- `bare` comes from `core.bare` in the shared config (`is_bare`), so it's correct even inside a linked worktree of a bare repo — unlike `--is-bare-repository`, which is false there.
- `main` is the repository's main worktree (first entry of `git worktree list --porcelain`), from which `name` and the worktree base are derived.
- `worktree_base(bare, main)` is what a `worktree_path` template resolves against: parent of the main worktree (normal), or the bare dir itself (bare, so worktrees land as siblings inside `barerepo.git/`).
- Default path template: `{repo}-{branch}` (sibling of the working tree) normal, `{branch}` bare. `/` in a branch name is flattened to `-` for the path only (git/hooks still get the real name).
- Gotchas: `git rev-parse --show-toplevel` *errors* (not empty) in a bare repo, so it's gated on `--is-bare-repository`; `--git-common-dir` returns `.`, so `git_dir` is canonicalized to avoid `/./`; under `--separate-git-dir` git reports the git dir (not the checkout) as the main worktree, so `name` is best-effort there (bare-ness is still correct via `core.bare`).

**Config** (`Config::load`) is TOML merged over a chain, later wins per key (mirrors git's system < global < local): `~/.config/tonic/config.toml` → `<repo>/tonic.toml` → `<gitdir>/tonic.toml`. The middle (shared, committed) slot is skipped for bare repos. Merge is per-top-level-key replace, not deep.

**`.worktreeinclude` transfer** (`transfer_includes`): source dir comes from `resolve_source` — the working tree normally, or for a bare-dir invocation the `main/` then `master/` worktree. Each line is a glob (top-level/relative, via the `glob` crate — *not* full gitignore semantics). A file is transferred only if it is **both** matched and gitignored (`is_ignored`); tracked files are never copied. Per-pattern `mode` (copy/symlink, default copy) comes from config `[[include]]`, keyed by matching the `.worktreeinclude` pattern string.

**`add` branch resolution** (see `add`): local branch → check out (error if already checked out elsewhere — one branch, one worktree); else a remote with the branch → local tracking branch (`choose_remote`: `origin` preferred, `--remote` to disambiguate, matches existing `refs/remotes/*` only — no fetch); else new branch from HEAD. `-b`/`--base` force a new branch and skip the remote step.

**`rm`/`cd` worktree resolution** (`resolve_worktree`): a worktree is a directory, not its current branch, so resolution matches in order — the branch currently checked out, then the path `add` would use for the name (`worktree_path_for`, the same computation `add` runs), then the worktree's dir basename. This keeps `rm`/`cd` working when a worktree's HEAD is detached or switched (e.g. after `gh stack checkout`).

**`list` stack lineage** (`lineage`): the list stays flat (worktrees are sibling dirs, not a tree). A row is annotated with a root-anchored chain `root → … → branch`, continued into its direct child **branches** — ` → child` for one, ` → [N]` for a fork — shown when the branch has an ancestor above the trunk or a child of its own. Children are branch-scoped (a child stacked on a branch shows even without its own worktree, since a stack is navigated by branch), found via `git branch --contains` (`direct_children`) filtered to immediate children; capped (`CHILDREN_FILTER_CAP`) so a pathological descendant set falls back to a count. Co-located tips (an empty branch twinning a real one) are deduped per commit, preferring a branch with a worktree. Lineage is *inferred* from the commit graph — one bounded `rev-list --first-parent --boundary <default>..<branch>` per worktree (cost = stack height, not branch count or history depth; `--boundary` + skipping default's tip commit handle a base whose tip has drifted into main), matched against a `sha → branch` tip map (`branch_tips`) — **no stored state**. `list` is two passes: pass 1 computes each worktree branch's lineage (memoized in `lineage_cached`) and its direct children, pass 2 renders. Render is shortened (#58): paths are relative to the worktree base (`short_path`), the current worktree is marked `▸`, and the row's own branch (the chain's last element) is rendered as `*` — a footnote back-ref to its label, freeing the name from being printed twice (`main → * → child` on a parent's row, `main → a → *` on the tip's). Each row also carries a trailing status (#30): compact `!N` (changed **file** count — an `MM` file counts once, via `Status::files` = one per porcelain line, not the sum of the three columns) in yellow + `↑N`/`↓N` ahead/behind in cyan, or with `-v`/`--verbose` the split `+staged *unstaged ?untracked` and full lineage names (no `*`). `worktree_status` parses `git status --porcelain` XY codes into a `Status`; `ahead_behind` runs `rev-list --left-right --count @{u}...HEAD` (returns `None` when there's no upstream); `format_dirty`/`format_upstream` render the two parts separately so they carry distinct colors. (The full serializable per-row model that unifies these with the lineage — the #59 seam #13 will serialize — is deferred to #13, which knows its output shape; #30 renders inline since the loop already holds the other fields.) An empty branch at its base's commit shows no lineage until it gets a commit (transient, self-healing). Rebase/re-parenting is out of scope; navigation is `up`/`down`/`top`/`bottom` (see below).

**Vertical stack navigation** (`up`/`down`/`top`/`bottom`, #43): moves along the same inferred lineage `list` uses — no new source of truth. `down`/`bottom` descend toward the trunk (parent side): `parent_of` = the predecessor of the current branch in its `lineage` chain; `base_of` = `chain[1]` (the branch on the trunk). `up`/`top` climb toward the tip (child side) via `direct_children`: `child_of` = the single child (a fork errors, naming them); `tip_of` walks single children up until a leaf. The novel bit is `goto`'s **cd-or-checkout**: if the target branch has a worktree, print its path so the shell wrapper `cd`s there (lateral); else `git checkout` it in place in the current worktree (vertical) — so the wrapper's command list (`shell_wrapper`) includes `up|down|top|bottom`, and only the cd branch prints stdout. Current branch comes from `symbolic-ref --short HEAD` (`current_branch`; detached or bare-dir invocation errors). Reaching an end of the stack is a no-op (`NavStep::Stay`, message to stderr, exit 0), not an error. No rebase/re-parenting — structure and movement only.

**Hooks** run via `sh -c` in the new worktree dir. Events: `post_create` (after add), `pre_remove` (before rm). Command strings are templated with `{repo}`, `{branch}`, `{worktree_path}` (see `render`).

**Output contract.** Human status goes to **stderr**; **stdout** carries machine-readable output only — the resolved worktree path for `add`/`cd`, the listing for `list`, and (only when removing the *current* worktree) a fallback path for `rm` so the wrapper can `cd` out of the doomed dir. `git_run` and hooks route their child stdout to stderr (`redirect_stdout_to_stderr`, unix-only) because e.g. `git worktree add` prints "HEAD is now at …" to stdout. This is what lets the `shell-init` wrapper do `cd "$(tonic add …)"` — so don't `println!` status text, use `eprintln!`.

## Packaging & release

The crate is **published as `git-tonic`** (the name `tonic` was taken on crates.io by the gRPC crate); the installed binary is still `tonic` via `[[bin]]`. Keep `[lib] name = "tonic"` so `main.rs`'s `tonic::` paths resolve.

Releases are cut by pushing a `vX.Y.Z` tag (`.github/workflows/release.yml`): it builds macOS/Linux × x86_64/aarch64 binaries (aarch64-linux via `cross`), attaches them to a GitHub Release, runs `cargo publish`, and bumps `Formula/tonic.rb` in `thombruce/homebrew-tap`. Needs repo secrets `CARGO_REGISTRY_TOKEN` and `HOMEBREW_TAP_TOKEN`. Bump `version` in `Cargo.toml` to match the tag before tagging.

## Testing conventions

Two layers (both run by `cargo test`):
- **Unit tests** in `lib.rs` for pure logic (`render`, `Config::merge`, `worktree_base`, `parse_worktrees`, `stack_order`-style helpers). The `tests` module opts out of the panic-lints.
- **Integration tests** in `tests/` (`#50`) drive the built binary against real scratch repos built in a tempdir — the `tests/common/mod.rs` `Scratch` helper (`git init`, `commit_in`, `tonic`/`tonic_in`, `wt(branch)` for the sibling worktree path). This is where git-shelling behaviour is tested — lineage/children (`list`), branch resolution (`add`), worktree resolution (`rm`/`cd`). **When you touch a git-shelling path, add or extend an integration test** rather than "verify by hand"; the seeded cases (drift, twin, empty-branch, deep stack, detached `rm`, remote tracking) are the regressions hand-verification missed. Uses `CARGO_BIN_EXE_tonic` (no `assert_cmd` dep) and `NO_COLOR=1` so output is plain.

When asserting `.worktreeinclude` behavior, note that `git worktree add` checks out *tracked* files itself, so a tracked file's presence in a worktree is not evidence tonic copied it — the guardrail only governs what tonic transfers.
