//! `tonic add` branch-resolution integration tests (#50).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::string_slice
)]

mod common;
use common::{stderr, Scratch};

#[test]
fn add_creates_a_new_branch_and_worktree() {
    let s = Scratch::new();
    let out = s.tonic(&["add", "newb"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(s.wt("newb").exists(), "worktree dir not created");
    assert!(s.git(&["branch", "--list", "newb"]).contains("newb"), "branch not created");
}

#[test]
fn add_checks_out_an_existing_local_branch() {
    let s = Scratch::new();
    s.git(&["branch", "existing"]);
    let out = s.tonic(&["add", "existing"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(s.wt("existing").exists());
}

#[test]
fn add_errors_when_branch_already_checked_out() {
    let s = Scratch::new();
    // main is checked out in the main worktree
    let out = s.tonic(&["add", "main"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("already checked out"),
        "unexpected error:\n{}",
        stderr(&out)
    );
}

#[test]
fn base_roots_a_new_branch_at_the_given_ref() {
    let s = Scratch::new();
    s.tonic(&["add", "feature"]);
    s.commit_in(&s.wt("feature"), "fc"); // feature is ahead of main
    // from feature's worktree, branch off main explicitly
    let out = s.tonic_in(&s.wt("feature"), &["add", "newb", "--base", "main"]);
    assert!(out.status.success(), "{}", stderr(&out));
    // subjects only — `--oneline` includes the SHA, whose hex can contain "fc"
    // and spuriously match (a flaky-in-CI substring collision).
    let log = s.git_in(&s.wt("newb"), &["log", "--format=%s"]);
    assert!(!log.contains("fc"), "newb should not include feature's commit:\n{log}");
}

#[test]
fn add_tracks_a_remote_only_branch() {
    let s = Scratch::new();
    // set up a bare origin and a branch that exists only on the remote
    let origin = s.root_join("origin.git");
    s.git(&["clone", "--bare", "-q", ".", origin.to_str().unwrap()]);
    s.git(&["remote", "add", "origin", origin.to_str().unwrap()]);
    s.git(&["branch", "feature/x"]);
    s.git(&["push", "-q", "origin", "feature/x"]);
    s.git(&["branch", "-D", "feature/x"]);
    s.git(&["fetch", "-q", "origin"]);

    let out = s.tonic(&["add", "feature/x"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("tracking origin/feature/x"),
        "expected remote tracking:\n{}",
        stderr(&out)
    );
}
