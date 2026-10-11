//! A worktree whose folder was deleted by hand but not yet pruned (#105): the
//! path-printing commands must not hand the shell wrapper a dir it can't `cd`
//! into, and `rm` must still be able to clean it up.
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

/// `main → a → b`, each with a worktree, then a's folder deleted by hand.
fn stack_with_a_missing(s: &Scratch) {
    s.tonic(&["add", "a"]);
    s.commit_in(&s.wt("a"), "ac");
    s.tonic_in(&s.wt("a"), &["add", "b"]);
    s.commit_in(&s.wt("b"), "bc");
    std::fs::remove_dir_all(s.wt("a")).unwrap();
}

fn assert_missing_refused(out: &std::process::Output) {
    assert!(!out.status.success(), "should refuse");
    assert!(
        stdout(out).trim().is_empty(),
        "no path for the wrapper to cd to"
    );
    assert!(stderr(out).contains("is missing"), "{}", stderr(out));
    assert!(stderr(out).contains("tonic rm a"), "{}", stderr(out));
}

#[test]
fn cd_checkout_and_nav_refuse_a_missing_folder() {
    let s = Scratch::new();
    stack_with_a_missing(&s);
    assert_missing_refused(&s.tonic(&["cd", "a"]));
    assert_missing_refused(&s.tonic(&["checkout", "a"]));
    assert_missing_refused(&s.tonic_in(&s.wt("b"), &["down"]));
    // nothing fell back to an in-place checkout
    assert_eq!(s.git_in(&s.wt("b"), &["branch", "--show-current"]), "b");
}

#[test]
fn rm_cleans_up_a_missing_folder_despite_pre_remove_hooks() {
    let s = Scratch::new();
    std::fs::write(
        s.repo.join("tonic.toml"),
        "[[hooks]]\nevent = \"pre_remove\"\nrun = \"true\"\n",
    )
    .unwrap();
    stack_with_a_missing(&s);
    // the hook can't run inside a deleted folder; it's skipped, not fatal
    let out = s.tonic(&["rm", "a"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("skipping pre_remove"),
        "{}",
        stderr(&out)
    );
    let list = s.git(&["worktree", "list"]);
    assert!(
        !list.contains("repo-a"),
        "worktree still registered:\n{list}"
    );
}

#[test]
fn rm_current_skips_a_parent_whose_folder_is_missing() {
    let s = Scratch::new();
    stack_with_a_missing(&s);
    // removing b from inside: its parent a has a registered but deleted worktree,
    // so return to the trunk instead of a path the wrapper can't cd into
    let out = s.tonic_in(&s.wt("b"), &["rm", "b", "-f"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).trim().ends_with("/repo"),
        "should fall back to the trunk:\n{}",
        stdout(&out)
    );
}

#[test]
fn rm_of_a_missing_folder_still_cleans_up_an_empty_branch() {
    let s = Scratch::new();
    s.tonic(&["add", "e"]); // no commits of its own → empty, auto-cleaned on rm
    std::fs::remove_dir_all(s.wt("e")).unwrap();
    let out = s.tonic(&["rm", "e"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        s.git(&["branch", "--list", "e"]),
        "",
        "empty branch left behind"
    );
}

#[test]
fn bare_rm_current_never_returns_a_missing_or_removed_worktree() {
    let s = Scratch::bare();
    s.tonic(&["add", "a"]);
    std::fs::remove_dir_all(&s.repo).unwrap(); // the trunk's main/ folder, by hand
    // removing a from inside: main/ is gone and a is the one being removed, so
    // the only place left to land is the bare dir itself
    let out = s.tonic_in(&s.wt("a"), &["rm", "a"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let back = std::path::PathBuf::from(stdout(&out).trim());
    assert!(
        back.exists(),
        "return path doesn't exist: {}",
        back.display()
    );
    assert_eq!(
        back.canonicalize().unwrap(),
        s.bare_dir().canonicalize().unwrap()
    );
}
