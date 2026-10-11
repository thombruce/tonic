# Changelog

Notable changes per release. Earlier releases: see the [GitHub releases](https://github.com/thombruce/tonic/releases).

## 0.4.0 (2026-10-11)

Three new ways to get to work: switch to any branch wherever it lives, open a
pull request in its own worktree, and take a detached snapshot of a tag or
commit. Plus fixes for tag-named branches and hand-deleted worktree folders.

### New

**`tonic checkout <branch>`**: put me on this branch, wherever it lives (#89, #108).
- If the branch has a worktree, tonic `cd`s you there.
- If not, it checks the branch out in place in your current worktree.
- A branch that exists only on a remote is created locally, tracking it, the
  same way `tonic add` resolves it. `--remote <name>` picks among several.
- An in-place checkout refuses when you have uncommitted changes to tracked
  files, so your work is never silently carried onto another branch.

**`tonic add --pr <N>`**: open pull request N in its own worktree, ready to
review or amend (#27).
- tonic makes the worktree and `gh` checks the PR out inside it, so tracking is
  set up and `git push` works, even for a PR from a fork (where the author
  allows maintainer edits).
- The folder is `repo-pr-<N>`, so `tonic cd pr-<N>` and `tonic rm pr-<N>` work.
  Set `pr_worktree_path` (placeholders `{repo}`, `{branch}`, `{pr}`) to name it
  differently.
- The local branch is the PR's head branch for a PR from the same repo, and
  `pr-<N>` for a PR from a fork.
- If the PR is already checked out in a worktree, tonic `cd`s you there.
- GitHub only for now; needs `gh` installed and signed in.

**`tonic add --detach <ref>`**: a worktree at a tag, SHA or ref, with no
branch (#107). For reading a release, reproducing a bug at a version, or
building an old version beside your current work.
- `.worktreeinclude` files are copied and `post_create` hooks run, as for a
  branch worktree. In hooks, `{branch}` is the ref as you typed it.
- The folder is named after the ref (`repo-v1.2`), so `tonic cd v1.2` and
  `tonic rm v1.2` find it.
- A local branch name is refused, because `cd`/`rm` would find that branch's
  worktree instead. Use `heads/<name>` or a SHA for a snapshot of a branch.

### Changed

- The current worktree and branch are now marked `»` in `tonic list` and
  `tonic stack`, instead of `▸` (#103).

### Fixed

- `tonic add v1.2`, where `v1.2` is a tag, used to create a new branch named
  `v1.2` at your current commit, not at the tag, leaving the name ambiguous.
  Names that already refer to a tag, SHA or other ref are now refused, with
  pointers to `--detach` or `--base` (#106).
- A worktree whose folder was deleted by hand is handled (#105):
  - `cd`, `checkout`, `up`/`down`/`top`/`bottom` and `add --pr` used to print a
    path that no longer existed. They now report the missing folder and suggest
    `tonic rm`.
  - `tonic rm` used to be blocked by a `pre_remove` hook, because the hook runs
    inside the folder. It now cleans up the leftover entry, skipping the hooks
    with a warning, and still deletes an empty branch.
  - Removing the worktree you're in never sends you to a missing folder or to
    the worktree being removed.

### Pull requests

#103, #104, #109, #110, #111, #115, #118

**Full Changelog**: https://github.com/thombruce/tonic/compare/v0.3.1...v0.4.0
