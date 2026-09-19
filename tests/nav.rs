//! `tonic up`/`down`/`top`/`bottom` vertical stack-navigation tests (#43).
//! The novel mechanism: a step cd's to the target's worktree if it has one,
//! else checks the branch out in place.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::string_slice
)]

mod common;
use common::{stderr, stdout, Scratch};

/// `main → a → b`, each with a worktree of its own.
fn stack_ab(s: &Scratch) {
    s.tonic(&["add", "a"]);
    s.commit_in(&s.wt("a"), "ac");
    s.tonic_in(&s.wt("a"), &["add", "b"]);
    s.commit_in(&s.wt("b"), "bc");
}

/// The branch currently checked out in worktree `dir`.
fn head(s: &Scratch, dir: &std::path::Path) -> String {
    s.git_in(dir, &["branch", "--show-current"])
}

#[test]
fn down_steps_to_parent_worktree() {
    let s = Scratch::new();
    stack_ab(&s);
    // from b, down → a's worktree (a has one → cd, path on stdout)
    let out = s.tonic_in(&s.wt("b"), &["down"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).trim().ends_with("repo-a"),
        "down should cd to a's worktree:\n{}",
        stdout(&out)
    );
}

#[test]
fn up_steps_to_child_worktree() {
    let s = Scratch::new();
    stack_ab(&s);
    let out = s.tonic_in(&s.wt("a"), &["up"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).trim().ends_with("repo-b"),
        "up should cd to b's worktree:\n{}",
        stdout(&out)
    );
}

#[test]
fn down_from_base_goes_to_trunk() {
    let s = Scratch::new();
    s.tonic(&["add", "a"]);
    s.commit_in(&s.wt("a"), "ac");
    // a is the base (main → a); down → main, which has the main worktree
    let out = s.tonic_in(&s.wt("a"), &["down"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out).trim(), s.repo.to_string_lossy(), "down from base should reach the trunk worktree");
}

#[test]
fn up_checks_out_in_place_when_child_has_no_worktree() {
    let s = Scratch::new();
    s.tonic(&["add", "foo"]);
    s.commit_in(&s.wt("foo"), "fc");
    // bar: stacked on foo, has a commit, but no worktree of its own
    let foo = s.wt("foo");
    s.git_in(&foo, &["branch", "bar"]);
    s.git_in(&foo, &["switch", "-q", "bar"]);
    s.commit_in(&foo, "bc");
    s.git_in(&foo, &["switch", "-q", "foo"]);

    let out = s.tonic_in(&foo, &["up"]);
    assert!(out.status.success(), "{}", stderr(&out));
    // in-place checkout: nothing on stdout (no cd), foo's worktree now on bar
    assert!(stdout(&out).trim().is_empty(), "in-place checkout should not print a path:\n{}", stdout(&out));
    assert_eq!(head(&s, &foo), "bar", "up should have checked bar out in place");
}

#[test]
fn top_jumps_to_the_tip() {
    let s = Scratch::new();
    stack_ab(&s);
    s.tonic_in(&s.wt("b"), &["add", "c"]);
    s.commit_in(&s.wt("c"), "cc");
    // from a, top → c (the tip)
    let out = s.tonic_in(&s.wt("a"), &["top"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).trim().ends_with("repo-c"), "top should reach the tip:\n{}", stdout(&out));
}

#[test]
fn bottom_jumps_to_the_base() {
    let s = Scratch::new();
    stack_ab(&s);
    s.tonic_in(&s.wt("b"), &["add", "c"]);
    s.commit_in(&s.wt("c"), "cc");
    // from c, bottom → a (the branch on the trunk)
    let out = s.tonic_in(&s.wt("c"), &["bottom"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).trim().ends_with("repo-a"), "bottom should reach the base:\n{}", stdout(&out));
}

#[test]
fn up_at_the_tip_is_a_noop() {
    let s = Scratch::new();
    stack_ab(&s);
    // b is the tip; up has nowhere to go
    let out = s.tonic_in(&s.wt("b"), &["up"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).trim().is_empty(), "no-op should not print a path");
    assert!(stderr(&out).contains("top of the stack"), "expected an at-the-top message:\n{}", stderr(&out));
}

#[test]
fn down_on_the_trunk_is_a_noop() {
    let s = Scratch::new();
    stack_ab(&s);
    // the main worktree is on the trunk; down has nowhere to go
    let out = s.tonic(&["down"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).trim().is_empty(), "no-op should not print a path");
    assert!(stderr(&out).contains("trunk"), "expected an on-the-trunk message:\n{}", stderr(&out));
}

/// foo with a childless branch `bar` stacked on it (no worktree), so `up` from
/// foo would check `bar` out in place. Returns foo's worktree path.
fn foo_with_in_place_child(s: &Scratch) -> std::path::PathBuf {
    s.tonic(&["add", "foo"]);
    s.commit_in(&s.wt("foo"), "fc");
    let dir = s.wt("foo");
    s.git_in(&dir, &["branch", "bar"]);
    s.git_in(&dir, &["switch", "-q", "bar"]);
    s.commit_in(&dir, "bc");
    s.git_in(&dir, &["switch", "-q", "foo"]);
    dir
}

#[test]
fn up_with_tracked_changes_is_refused() {
    let s = Scratch::new();
    let dir = foo_with_in_place_child(&s);
    // modify a *tracked* file — an in-place checkout would carry it onto bar
    std::fs::write(dir.join("fc"), "edited").unwrap();

    let out = s.tonic_in(&dir, &["up"]);
    assert!(!out.status.success(), "in-place checkout should be refused with tracked changes");
    assert!(
        stderr(&out).contains("tracked files"),
        "expected a tracked-changes error:\n{}",
        stderr(&out)
    );
    assert_eq!(head(&s, &dir), "foo", "HEAD must be unchanged after a refused checkout");
}

#[test]
fn up_with_only_untracked_changes_is_allowed() {
    let s = Scratch::new();
    let dir = foo_with_in_place_child(&s);
    // an untracked file carries across harmlessly — git allows it, so must we (#63)
    std::fs::write(dir.join("scratch"), "wip").unwrap();

    let out = s.tonic_in(&dir, &["up"]);
    assert!(out.status.success(), "untracked-only should not block checkout:\n{}", stderr(&out));
    assert_eq!(head(&s, &dir), "bar", "up should have checked bar out in place");
    // the untracked file is still there, undisturbed
    assert!(dir.join("scratch").exists(), "untracked file should carry across");
}

#[test]
fn detached_head_errors() {
    let s = Scratch::new();
    stack_ab(&s);
    s.git_in(&s.wt("b"), &["switch", "-q", "--detach", "HEAD"]);
    let out = s.tonic_in(&s.wt("b"), &["down"]);
    assert!(!out.status.success(), "navigation needs a branch checked out");
    assert!(stderr(&out).contains("detached"), "expected a detached-HEAD error:\n{}", stderr(&out));
}

#[test]
fn up_into_a_fork_errors_naming_the_children() {
    let s = Scratch::new();
    s.tonic(&["add", "a"]);
    s.commit_in(&s.wt("a"), "ac");
    let a = s.wt("a");
    // two children of a, each with a commit, no worktrees
    for child in ["b", "c"] {
        s.git_in(&a, &["switch", "-q", "-c", child]);
        s.commit_in(&a, &format!("{child}c"));
        s.git_in(&a, &["switch", "-q", "a"]);
    }
    let out = s.tonic_in(&a, &["up"]);
    assert!(!out.status.success(), "a fork should not silently pick a branch");
    let err = stderr(&out);
    assert!(err.contains("forks"), "expected a fork error:\n{err}");
    assert!(err.contains('b') && err.contains('c'), "fork error should name the children:\n{err}");
}
