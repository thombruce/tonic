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

**Hooks** run via `sh -c` in the new worktree dir. Events: `post_create` (after add), `pre_remove` (before rm). Command strings are templated with `{repo}`, `{branch}`, `{worktree_path}` (see `render`).

**Output contract.** Human status goes to **stderr**; **stdout** carries machine-readable output only — the resolved worktree path for `add`/`cd`, the listing for `list`, and (only when removing the *current* worktree) a fallback path for `rm` so the wrapper can `cd` out of the doomed dir. `git_run` and hooks route their child stdout to stderr (`redirect_stdout_to_stderr`, unix-only) because e.g. `git worktree add` prints "HEAD is now at …" to stdout. This is what lets the `shell-init` wrapper do `cd "$(tonic add …)"` — so don't `println!` status text, use `eprintln!`.

## Packaging & release

The crate is **published as `git-tonic`** (the name `tonic` was taken on crates.io by the gRPC crate); the installed binary is still `tonic` via `[[bin]]`. Keep `[lib] name = "tonic"` so `main.rs`'s `tonic::` paths resolve.

Releases are cut by pushing a `vX.Y.Z` tag (`.github/workflows/release.yml`): it builds macOS/Linux × x86_64/aarch64 binaries (aarch64-linux via `cross`), attaches them to a GitHub Release, runs `cargo publish`, and bumps `Formula/tonic.rb` in `thombruce/homebrew-tap`. Needs repo secrets `CARGO_REGISTRY_TOKEN` and `HOMEBREW_TAP_TOKEN`. Bump `version` in `Cargo.toml` to match the tag before tagging.

## Testing conventions

Pure logic (`render`, `Config::merge`, `worktree_base`, `parse_worktrees`) has unit tests in `lib.rs`. Code that shells out to git is not unit-tested — verify it by hand against a scratch repo (create a temp git repo, or `git clone --bare` for the bare paths, and run the built binary). When asserting `.worktreeinclude` behavior, note that `git worktree add` checks out *tracked* files itself, so a tracked file's presence in a worktree is not evidence tonic copied it — the guardrail only governs what tonic transfers.
