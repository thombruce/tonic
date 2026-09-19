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
use common::{stderr, stdout, Scratch};

/// The repo's local branch names.
fn branches(s: &Scratch) -> Vec<String> {
    s.git(&["branch", "--format=%(refname:short)"])
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn rm_auto_deletes_an_empty_branch() {
    let s = Scratch::new();
    s.tonic(&["add", "scratch"]); // no commit → empty branch
    let out = s.tonic(&["rm", "scratch"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        !branches(&s).iter().any(|b| b == "scratch"),
        "an empty branch should be auto-deleted on rm:\n{:?}",
        branches(&s)
    );
}

#[test]
fn rm_keeps_a_branch_that_has_commits() {
    let s = Scratch::new();
    s.tonic(&["add", "work"]);
    s.commit_in(&s.wt("work"), "wc"); // real commit → not empty
    let out = s.tonic(&["rm", "work"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        branches(&s).iter().any(|b| b == "work"),
        "a branch with commits must be kept (needs explicit -d):\n{:?}",
        branches(&s)
    );
}

#[test]
fn rm_keeps_a_stacked_branch_with_no_own_commit() {
    let s = Scratch::new();
    s.tonic(&["add", "a"]);
    s.commit_in(&s.wt("a"), "ac");
    s.tonic_in(&s.wt("a"), &["add", "b"]); // off a, no own commit — but a's commit is beyond trunk
    let out = s.tonic(&["rm", "b"]);
    assert!(out.status.success(), "{}", stderr(&out));
    // conservative: b carries commits beyond the trunk (a's), so it's not "empty"
    assert!(
        branches(&s).iter().any(|b| b == "b"),
        "a stacked branch (commits beyond trunk) must be kept:\n{:?}",
        branches(&s)
    );
}

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
