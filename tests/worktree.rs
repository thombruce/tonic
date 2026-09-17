//! `tonic rm` / `tonic cd` worktree-resolution integration tests (#50).
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

#[test]
fn rm_removes_a_worktree() {
    let s = Scratch::new();
    s.tonic(&["add", "foo"]);
    assert!(s.wt("foo").exists());
    let out = s.tonic(&["rm", "foo", "-f"]);
    assert!(out.status.success());
    assert!(!s.wt("foo").exists(), "worktree not removed");
}

#[test]
fn rm_resolves_a_detached_worktree_by_name() {
    let s = Scratch::new();
    s.tonic(&["add", "foo"]);
    // detach HEAD in the worktree: its branch label no longer matches "foo"
    s.git_in(&s.wt("foo"), &["switch", "-q", "--detach", "HEAD"]);
    let out = s.tonic(&["rm", "foo", "-f"]);
    assert!(out.status.success(), "rm couldn't resolve a detached worktree");
    assert!(!s.wt("foo").exists());
}

#[test]
fn cd_prints_the_worktree_path() {
    let s = Scratch::new();
    s.tonic(&["add", "foo"]);
    let out = s.tonic(&["cd", "foo"]);
    assert!(out.status.success());
    assert!(
        stdout(&out).trim().ends_with("repo-foo"),
        "cd printed unexpected path:\n{}",
        stdout(&out)
    );
}

#[test]
fn rm_of_the_current_worktree_prints_a_fallback_path() {
    let s = Scratch::new();
    s.tonic(&["add", "foo"]);
    // remove foo while standing in it — stdout should carry a path to cd to
    let out = s.tonic_in(&s.wt("foo"), &["rm", "foo", "-f"]);
    assert!(out.status.success());
    let path = stdout(&out);
    assert!(!path.trim().is_empty(), "no fallback path printed");
    assert!(path.contains("repo"), "fallback should be the main worktree:\n{path}");
}
