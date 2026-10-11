//! `tonic checkout <branch>` (#89): cd-or-checkout for any branch — the
//! generalisation of the nav commands' `goto` beyond stack moves.
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

#[test]
fn branch_with_a_worktree_prints_its_path() {
    let s = Scratch::new();
    s.tonic(&["add", "a"]);
    let out = s.tonic(&["checkout", "a"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        std::path::Path::new(stdout(&out).trim()),
        s.wt("a").canonicalize().unwrap()
    );
    // lateral move: the invoking worktree is untouched
    assert_eq!(s.git(&["branch", "--show-current"]), "main");
}

#[test]
fn branch_without_a_worktree_checks_out_in_place() {
    let s = Scratch::new();
    s.git(&["branch", "x"]);
    let out = s.tonic(&["checkout", "x"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).trim().is_empty(), "in-place prints no path");
    assert_eq!(s.git(&["branch", "--show-current"]), "x");
}

#[test]
fn in_place_refuses_tracked_changes() {
    let s = Scratch::new();
    s.git(&["branch", "x"]);
    std::fs::write(s.repo.join("m0"), "edited").unwrap();
    let out = s.tonic(&["checkout", "x"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("uncommitted"), "{}", stderr(&out));
    assert_eq!(s.git(&["branch", "--show-current"]), "main");
}

#[test]
fn bare_dir_cds_but_wont_check_out_in_place() {
    let s = Scratch::bare();
    s.tonic(&["add", "a"]);
    let out = s.tonic_in(&s.bare_dir(), &["checkout", "a"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).trim().ends_with("a"), "{}", stdout(&out));

    s.git(&["branch", "x"]);
    let out = s.tonic_in(&s.bare_dir(), &["checkout", "x"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no worktree"), "{}", stderr(&out));
}

#[test]
fn refuses_a_name_that_is_not_a_local_branch() {
    let s = Scratch::new();
    s.git(&["tag", "t1"]);
    // m0 is a tracked file: git would restore it as a pathspec and exit 0;
    // t1 is a tag: git would detach HEAD.
    for name in ["m0", "t1", "nope"] {
        let out = s.tonic(&["checkout", name]);
        assert!(!out.status.success(), "{name} should be refused");
        assert!(
            stderr(&out).contains("not a local branch"),
            "{}",
            stderr(&out)
        );
        assert!(!stderr(&out).contains("tonic add"), "no remote, no hint");
        assert_eq!(s.git(&["branch", "--show-current"]), "main");
    }
}

/// Push branch `name` to a bare remote called `remote` (created if needed),
/// then drop the local copy, so `name` exists only as `<remote>/<name>`.
fn remote_only(s: &Scratch, remote: &str, name: &str) {
    let url = s.root_join(&format!("{remote}.git"));
    if !url.exists() {
        s.git(&["clone", "--bare", "-q", ".", url.to_str().unwrap()]);
        s.git(&["remote", "add", remote, url.to_str().unwrap()]);
    }
    if s.git(&["branch", "--list", name]).is_empty() {
        s.git(&["branch", name]);
    }
    s.git(&["push", "-q", remote, name]);
    s.git(&["branch", "-D", name]);
    s.git(&["fetch", "-q", remote]);
}

#[test]
fn remote_only_branch_is_created_tracking_in_place() {
    let s = Scratch::new();
    remote_only(&s, "origin", "r");
    let out = s.tonic(&["checkout", "r"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).trim().is_empty(), "in-place prints no path");
    assert_eq!(s.git(&["branch", "--show-current"]), "r");
    assert_eq!(s.git(&["rev-parse", "--abbrev-ref", "r@{u}"]), "origin/r");
}

#[test]
fn remote_only_branch_on_several_remotes_needs_remote() {
    let s = Scratch::new();
    remote_only(&s, "up", "r");
    remote_only(&s, "fork", "r");
    // neither is origin: ambiguous, so name them and point at --remote
    let out = s.tonic(&["checkout", "r"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("--remote"), "{}", stderr(&out));
    assert_eq!(s.git(&["branch", "--show-current"]), "main");

    let out = s.tonic(&["checkout", "r", "--remote", "fork"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(s.git(&["rev-parse", "--abbrev-ref", "r@{u}"]), "fork/r");
}

#[test]
fn remote_checkout_keeps_the_guards() {
    let s = Scratch::new();
    remote_only(&s, "origin", "r");
    // dirty tracked file: refused, nothing created
    std::fs::write(s.repo.join("m0"), "edited").unwrap();
    let out = s.tonic(&["checkout", "r"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("uncommitted"), "{}", stderr(&out));
    assert_eq!(s.git(&["branch", "--list", "r"]), "", "no branch created");
    s.git(&["checkout", "-q", "--", "m0"]);

    // --remote with a branch that already exists locally
    let out = s.tonic(&["checkout", "main", "--remote", "origin"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("already exists locally"),
        "{}",
        stderr(&out)
    );
}
