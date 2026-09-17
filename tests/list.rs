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
    assert!(out.contains("main → a → b"), "expected full lineage, got:\n{out}");
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
    assert!(out.contains("main → A → B"), "drift regression: lineage lost:\n{out}");
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
    assert!(out.contains("foo → bar"), "non-worktree child not shown:\n{out}");
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
    assert!(out.contains("foo → bar"), "expected single child:\n{out}");
    assert!(!out.contains("[2]"), "twin inflated the fork count:\n{out}");
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
    assert!(after.contains("foo → empty"), "lineage should appear after a commit:\n{after}");
}
