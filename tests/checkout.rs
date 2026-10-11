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

#[test]
fn remote_only_branch_hints_at_add() {
    let s = Scratch::new();
    let origin = s.root_join("origin.git");
    s.git(&["clone", "--bare", "-q", ".", origin.to_str().unwrap()]);
    s.git(&["remote", "add", "origin", origin.to_str().unwrap()]);
    s.git(&["branch", "r"]);
    s.git(&["push", "-q", "origin", "r"]);
    s.git(&["branch", "-D", "r"]);
    s.git(&["fetch", "-q", "origin"]);

    let out = s.tonic(&["checkout", "r"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("tonic add r"), "{}", stderr(&out));
}
