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
use common::{stderr, stdout, Scratch};

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
fn new_branch_from_a_detached_head_warns_and_roots_at_the_commit() {
    let s = Scratch::new();
    s.tonic(&["add", "base"]);
    s.commit_in(&s.wt("base"), "bc");
    let base = s.wt("base");
    // detach the worktree's HEAD (as `gh stack checkout` / a rebase would) — a new
    // branch with no --base then roots at this commit, not a named branch (#74).
    s.git_in(&base, &["switch", "-q", "--detach", "HEAD"]);
    let detached = s.git_in(&base, &["rev-parse", "HEAD"]);

    let out = s.tonic_in(&base, &["add", "child"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("detached"),
        "a new branch from a detached HEAD should warn:\n{}",
        stderr(&out)
    );
    // still created — from the detached commit
    assert_eq!(
        s.git_in(&s.wt("child"), &["rev-parse", "HEAD"]),
        detached,
        "new branch should be rooted at the detached commit"
    );
}

#[test]
fn new_branch_on_a_branch_does_not_warn() {
    let s = Scratch::new();
    s.tonic(&["add", "feature"]);
    s.commit_in(&s.wt("feature"), "fc");
    // on a named branch in a worktree — the normal case, no surprise, no warning
    let out = s.tonic_in(&s.wt("feature"), &["add", "child"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(
        !err.contains("detached") && !err.contains("no base"),
        "a normal new branch must not warn:\n{err}"
    );
}

#[test]
fn add_survives_a_failing_post_create_hook() {
    let s = Scratch::new();
    // a post_create hook that fails must not abort add — the worktree already
    // exists, and add must still print the path so the shell wrapper cd's in (#76).
    std::fs::write(
        s.repo.join("tonic.toml"),
        "[[hooks]]\nevent = \"post_create\"\nrun = \"exit 3\"\n",
    )
    .unwrap();
    let out = s.tonic(&["add", "feat"]);
    assert!(out.status.success(), "post_create failure must not abort add:\n{}", stderr(&out));
    assert!(s.wt("feat").exists(), "worktree should still be created");
    assert!(
        stdout(&out).trim().ends_with("repo-feat"),
        "the path must still print for the wrapper to cd:\n{}",
        stdout(&out)
    );
    assert!(stderr(&out).contains("hook failed"), "the failure should be surfaced:\n{}", stderr(&out));
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
