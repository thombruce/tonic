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

**`Repo::discover` is the model for normal vs bare repos.** `root: Option<PathBuf>` is `None` for a bare repo (no working tree). Because of this split:
- `Repo::cwd()` is the dir to run git subcommands from (working tree, or the bare dir).
- `worktree_base()` is what a `worktree_path` template resolves against (parent of the working tree for normal; the bare dir itself for bare, so worktrees land as siblings inside `barerepo.git/`).
- The default path template differs: `{repo}-{branch}` (sibling of the working tree) normal, `{branch}` bare. `/` in a branch name is flattened to `-` for the path only (git/hooks still get the real name).
- Gotchas encoded here: `git rev-parse --show-toplevel` *errors* (not empty) in a bare repo, so it's only called when not bare; `--git-common-dir` returns `.`, so `git_dir` is canonicalized to avoid `/./` in derived paths.

**Config** (`Config::load`) is TOML merged over a chain, later wins per key (mirrors git's system < global < local): `~/.config/tonic/config.toml` → `<repo>/tonic.toml` → `<gitdir>/tonic.toml`. The middle (shared, committed) slot is skipped for bare repos. Merge is per-top-level-key replace, not deep.

**`.worktreeinclude` transfer** (`transfer_includes`): source dir comes from `resolve_source` — the working tree normally, or for a bare-dir invocation the `main/` then `master/` worktree. Each line is a glob (top-level/relative, via the `glob` crate — *not* full gitignore semantics). A file is transferred only if it is **both** matched and gitignored (`is_ignored`); tracked files are never copied. Per-pattern `mode` (copy/symlink, default copy) comes from config `[[include]]`, keyed by matching the `.worktreeinclude` pattern string.

**Hooks** run via `sh -c` in the new worktree dir. Events: `post_create` (after add), `pre_remove` (before rm). Command strings are templated with `{repo}`, `{branch}`, `{worktree_path}` (see `render`).

## Packaging & release

The crate is **published as `git-tonic`** (the name `tonic` was taken on crates.io by the gRPC crate); the installed binary is still `tonic` via `[[bin]]`. Keep `[lib] name = "tonic"` so `main.rs`'s `tonic::` paths resolve.

Releases are cut by pushing a `vX.Y.Z` tag (`.github/workflows/release.yml`): it builds macOS/Linux × x86_64/aarch64 binaries (aarch64-linux via `cross`), attaches them to a GitHub Release, runs `cargo publish`, and bumps `Formula/tonic.rb` in `thombruce/homebrew-tap`. Needs repo secrets `CARGO_REGISTRY_TOKEN` and `HOMEBREW_TAP_TOKEN`. Bump `version` in `Cargo.toml` to match the tag before tagging.

## Testing conventions

Pure logic (`render`, `Config::merge`, `worktree_base`, `parse_worktrees`) has unit tests in `lib.rs`. Code that shells out to git is not unit-tested — verify it by hand against a scratch repo (create a temp git repo, or `git clone --bare` for the bare paths, and run the built binary). When asserting `.worktreeinclude` behavior, note that `git worktree add` checks out *tracked* files itself, so a tracked file's presence in a worktree is not evidence tonic copied it — the guardrail only governs what tonic transfers.
