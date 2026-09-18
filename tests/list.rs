//! `tonic list` stack-lineage integration tests — the cases that shipped as bugs
//! and which hand-verification missed (#50).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::string_slice
)]

mod common;
use common::{stdout, Scratch};

/// Build `main → a → b`: each branch created from the one below's worktree, with
/// a commit of its own.
fn linear_stack(s: &Scratch) {
    s.tonic(&["add", "a"]);
    s.commit_in(&s.wt("a"), "ac");
    s.tonic_in(&s.wt("a"), &["add", "b"]);
    s.commit_in(&s.wt("b"), "bc");
}

#[test]
fn deep_stack_shows_full_lineage() {
    let s = Scratch::new();
    linear_stack(&s);
    let out = stdout(&s.tonic(&["list"]));
    // each row shows its own branch as `*` (a footnote back-ref to its label):
    // b's row is `main → a → *`, a's row is `main → * → b`.
    assert!(out.contains("main → a → *"), "expected b's full lineage, got:\n{out}");
    assert!(out.contains("main → * → b"), "expected a's lineage with child, got:\n{out}");
}

#[test]
fn plain_branch_off_main_has_no_lineage() {
    let s = Scratch::new();
    s.tonic(&["add", "solo"]);
    s.commit_in(&s.wt("solo"), "sc");
    let out = stdout(&s.tonic(&["list"]));
    // a branch directly on the trunk is not a stack
    assert!(!out.contains('→'), "plain branch should not be annotated:\n{out}");
}

#[test]
fn lineage_survives_main_drifting_past_the_base() {
    let s = Scratch::new();
    s.tonic(&["add", "A"]);
    s.commit_in(&s.wt("A"), "a1");
    s.tonic_in(&s.wt("A"), &["add", "B"]);
    s.commit_in(&s.wt("B"), "b1");
    // advance main beyond A's tip (A now behind main)
    s.git(&["merge", "--ff-only", "A"]);
    s.commit_in(&s.repo, "m2");
    let out = stdout(&s.tonic(&["list"]));
    assert!(out.contains("main → A → *"), "drift regression: lineage lost:\n{out}");
}

#[test]
fn child_without_a_worktree_is_shown() {
    let s = Scratch::new();
    s.tonic(&["add", "foo"]);
    s.commit_in(&s.wt("foo"), "fc");
    // bar: a branch stacked on foo, with a commit, but no worktree of its own
    let foo = s.wt("foo");
    s.git_in(&foo, &["branch", "bar"]);
    s.git_in(&foo, &["switch", "-q", "bar"]);
    s.commit_in(&foo, "bc");
    s.git_in(&foo, &["switch", "-q", "foo"]);
    let out = stdout(&s.tonic(&["list"]));
    // foo's row: `main → * → bar` — bar (no worktree) still shown as foo's child.
    assert!(out.contains("* → bar"), "non-worktree child not shown:\n{out}");
}

#[test]
fn twin_child_is_not_a_phantom_fork() {
    let s = Scratch::new();
    s.tonic(&["add", "foo"]);
    s.commit_in(&s.wt("foo"), "fc");
    let foo = s.wt("foo");
    s.git_in(&foo, &["branch", "bar"]);
    s.git_in(&foo, &["switch", "-q", "bar"]);
    s.commit_in(&foo, "bc");
    s.git_in(&foo, &["switch", "-q", "foo"]);
    // an empty branch twinning bar at bar's commit
    s.git_in(&foo, &["branch", "bartwin", "bar"]);
    let out = stdout(&s.tonic(&["list"]));
    assert!(out.contains("* → bar"), "expected single child:\n{out}");
    assert!(!out.contains("[2]"), "twin inflated the fork count:\n{out}");
}

#[test]
fn dirty_count_replaces_the_dirty_word() {
    let s = Scratch::new();
    s.tonic(&["add", "foo"]);
    std::fs::write(s.wt("foo").join("wip"), "x").unwrap(); // 1 untracked
    let out = stdout(&s.tonic(&["list"]));
    assert!(out.contains("!1"), "expected a compact dirty count:\n{out}");
    assert!(!out.contains("(dirty)"), "the (dirty) word should be gone:\n{out}");
}

#[test]
fn verbose_splits_status_and_shows_full_lineage_names() {
    let s = Scratch::new();
    linear_stack(&s); // main → a → b
    std::fs::write(s.wt("b").join("wip"), "x").unwrap(); // 1 untracked on b
    let out = stdout(&s.tonic(&["list", "-v"]));
    // verbose splits the count and keeps full branch names (no `*` self-marker)
    assert!(out.contains("?1"), "verbose should split out untracked:\n{out}");
    assert!(out.contains("main → a → b"), "verbose should show full lineage names:\n{out}");
    assert!(!out.contains("→ *"), "verbose should not use the `*` self-marker:\n{out}");
}

#[test]
fn ahead_behind_shown_against_upstream() {
    let s = Scratch::new();
    let origin = s.root_join("origin.git");
    s.git(&["clone", "--bare", "-q", ".", origin.to_str().unwrap()]);
    s.git(&["remote", "add", "origin", origin.to_str().unwrap()]);
    s.git(&["branch", "feat"]);
    s.git(&["push", "-q", "origin", "feat"]);
    s.tonic(&["add", "feat"]);
    // feat tracks origin/feat; put it two commits ahead
    let feat = s.wt("feat");
    s.git_in(&feat, &["branch", "--set-upstream-to=origin/feat"]);
    s.commit_in(&feat, "f1");
    s.commit_in(&feat, "f2");
    let out = stdout(&s.tonic(&["list"]));
    assert!(out.contains("↑2"), "expected an ahead count:\n{out}");
}

#[test]
fn paths_are_relative_and_current_is_marked() {
    let s = Scratch::new();
    s.tonic(&["add", "a"]);
    let out = stdout(&s.tonic(&["list"]));
    // current worktree carries the `▸` marker (no longer `*`, which now marks
    // self-position in the chain).
    assert!(out.contains('▸'), "current worktree not marked:\n{out}");
    // paths are shown relative to the base, not as absolute paths.
    assert!(out.contains("repo-a"), "worktree dir name missing:\n{out}");
    assert!(
        !out.contains(&*s.repo.to_string_lossy()),
        "path should be relative, not the absolute worktree path:\n{out}"
    );
}

#[test]
fn configured_default_branch_anchors_lineage() {
    let s = Scratch::new();
    // develop advances past main, then a stack is built on it
    s.tonic(&["add", "develop"]);
    s.commit_in(&s.wt("develop"), "dc");
    s.tonic_in(&s.wt("develop"), &["add", "a"]);
    s.commit_in(&s.wt("a"), "ac");
    s.tonic_in(&s.wt("a"), &["add", "b"]);
    s.commit_in(&s.wt("b"), "bc");

    // without config, main is the trunk: develop reads as a stacked branch
    let out = stdout(&s.tonic(&["list"]));
    assert!(out.contains("main → develop → a → *"), "default trunk should be main:\n{out}");

    // with default_branch = develop, lineage anchors at develop instead
    std::fs::write(s.repo.join("tonic.toml"), "default_branch = \"develop\"\n").unwrap();
    let out = stdout(&s.tonic(&["list"]));
    assert!(out.contains("develop → a → *"), "configured trunk not honored:\n{out}");
    assert!(!out.contains("main → develop"), "develop should be the root, not a child:\n{out}");
}

#[test]
fn stale_default_branch_config_falls_back() {
    let s = Scratch::new();
    linear_stack(&s);
    // a branch that doesn't exist must not point lineage at a phantom ref
    std::fs::write(s.repo.join("tonic.toml"), "default_branch = \"nope\"\n").unwrap();
    let out = stdout(&s.tonic(&["list"]));
    assert!(out.contains("main → a → *"), "stale config should fall back to main:\n{out}");
}

#[test]
fn empty_branch_gains_lineage_after_a_commit() {
    let s = Scratch::new();
    s.tonic(&["add", "foo"]);
    s.commit_in(&s.wt("foo"), "fc");
    s.tonic_in(&s.wt("foo"), &["add", "empty"]); // off foo, no commit yet

    let before = stdout(&s.tonic(&["list"]));
    assert!(!before.contains("→ empty"), "empty branch should show no lineage yet:\n{before}");

    s.commit_in(&s.wt("empty"), "ec");
    let after = stdout(&s.tonic(&["list"]));
    // empty's row now shows `main → foo → *`.
    assert!(after.contains("main → foo → *"), "lineage should appear after a commit:\n{after}");
}
