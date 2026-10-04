//! `tonic stack` — the current worktree's stack as a vertical column (#82).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::string_slice
)]

mod common;
use common::{Scratch, stderr, stdout};

/// Line index of the first row naming `branch` (panics if absent).
fn row(out: &str, branch: &str) -> usize {
    out.lines()
        .position(|l| l.split_whitespace().any(|w| w == branch))
        .unwrap_or_else(|| panic!("no row for {branch} in:\n{out}"))
}

#[test]
fn stack_lists_the_chain_tip_to_trunk() {
    let s = Scratch::new();
    s.tonic(&["add", "a"]);
    s.commit_in(&s.wt("a"), "ac");
    s.tonic_in(&s.wt("a"), &["add", "b"]);
    s.commit_in(&s.wt("b"), "bc");

    let out = stdout(&s.tonic_in(&s.wt("b"), &["stack"]));
    // tip at top, trunk at bottom
    assert!(
        row(&out, "b") < row(&out, "a"),
        "tip should be above its parent:\n{out}"
    );
    assert!(
        row(&out, "a") < row(&out, "main"),
        "trunk should be at the bottom:\n{out}"
    );
    // current branch is marked
    assert!(
        out.lines().any(|l| l.contains('▸') && l.contains('b')),
        "current not marked:\n{out}"
    );
}

#[test]
fn stack_climbs_above_a_mid_stack_branch() {
    let s = Scratch::new();
    s.tonic(&["add", "a"]);
    s.commit_in(&s.wt("a"), "ac");
    s.tonic_in(&s.wt("a"), &["add", "b"]);
    s.commit_in(&s.wt("b"), "bc");
    // run from a (mid-stack): the single child b stacked above must render above it
    // (exercises the climb-toward-the-tip path, not just ancestors).
    let out = stdout(&s.tonic_in(&s.wt("a"), &["stack"]));
    assert!(
        row(&out, "b") < row(&out, "a"),
        "child should render above current:\n{out}"
    );
    assert!(
        row(&out, "a") < row(&out, "main"),
        "trunk should be at the bottom:\n{out}"
    );
    assert!(
        out.lines().any(|l| l.contains('▸') && l.contains('a')),
        "current branch a not marked:\n{out}"
    );
}

#[test]
fn stack_notes_a_fork() {
    let s = Scratch::new();
    s.tonic(&["add", "a"]);
    s.commit_in(&s.wt("a"), "ac");
    s.tonic_in(&s.wt("a"), &["add", "b"]); // child of a (worktree)
    s.commit_in(&s.wt("b"), "bc");
    // a second child of a, no worktree
    let a = s.wt("a");
    s.git_in(&a, &["switch", "-q", "-c", "c"]);
    s.commit_in(&a, "cc");
    s.git_in(&a, &["switch", "-q", "a"]);

    let out = stdout(&s.tonic_in(&a, &["stack"]));
    assert!(out.contains("forks into"), "a fork should be noted:\n{out}");
    assert!(
        out.contains('b') && out.contains('c'),
        "fork note should name the children:\n{out}"
    );
}

#[test]
fn stack_errors_on_a_detached_head() {
    let s = Scratch::new();
    s.tonic(&["add", "foo"]);
    s.git_in(&s.wt("foo"), &["switch", "-q", "--detach", "HEAD"]);
    let out = s.tonic_in(&s.wt("foo"), &["stack"]);
    assert!(!out.status.success(), "stack needs a branch checked out");
    assert!(
        stderr(&out).contains("detached"),
        "expected a detached-HEAD error:\n{}",
        stderr(&out)
    );
}
