//! `tonic add --detach <ref>` (#107): a detached worktree at a tag, SHA or ref,
//! with tonic's includes, hooks and naming — and no branch created.
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
fn detach_at_a_tag_makes_a_branchless_worktree_at_that_commit() {
    let s = Scratch::new();
    s.git(&["tag", "v1"]);
    let tagged = s.git(&["rev-parse", "HEAD"]);
    s.commit_in(&s.repo, "m1");

    let out = s.tonic(&["add", "--detach", "v1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).trim().ends_with("repo-v1"), "{}", stdout(&out));
    let wt = s.wt("v1");
    assert_eq!(s.git_in(&wt, &["rev-parse", "HEAD"]), tagged);
    assert_eq!(
        s.git_in(&wt, &["branch", "--show-current"]),
        "",
        "not detached"
    );
    assert_eq!(
        s.git(&["branch", "--list", "v1"]),
        "",
        "a branch was created"
    );

    // cd / rm find it by the name it was added under
    let out = s.tonic(&["cd", "v1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let out = s.tonic(&["rm", "v1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!wt.exists());
}

#[test]
fn detach_accepts_a_sha_and_a_slashed_ref() {
    let s = Scratch::new();
    let sha = s.git(&["rev-parse", "--short", "HEAD"]);
    let out = s.tonic(&["add", "--detach", &sha]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(s.wt(&sha).exists());

    // `/` is flattened for the path, as for branches
    s.git(&["update-ref", "refs/remotes/origin/x", "HEAD"]);
    let out = s.tonic(&["add", "--detach", "origin/x"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(s.wt("origin-x").exists());
}

#[test]
fn detach_hooks_get_the_ref_as_branch() {
    let s = Scratch::new();
    std::fs::write(
        s.repo.join("tonic.toml"),
        "[[hooks]]\nevent = \"post_create\"\nrun = \"echo {branch} > hook.out\"\n",
    )
    .unwrap();
    s.git(&["tag", "v1"]);
    let out = s.tonic(&["add", "--detach", "v1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let got = std::fs::read_to_string(s.wt("v1").join("hook.out")).unwrap();
    assert_eq!(got.trim(), "v1");
}

#[test]
fn detach_refuses_an_unknown_ref_and_new_branch_flags() {
    let s = Scratch::new();
    let out = s.tonic(&["add", "--detach", "nope"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("doesn't name a commit"),
        "{}",
        stderr(&out)
    );
    assert!(!s.wt("nope").exists());

    for flag in [
        ["-b"].as_slice(),
        &["--base", "main"],
        &["--remote", "origin"],
    ] {
        let mut args = vec!["add", "--detach", "x"];
        args.extend_from_slice(flag);
        let out = s.tonic(&args);
        assert!(!out.status.success(), "{args:?} should conflict");
        assert!(
            stderr(&out).contains("cannot be used with"),
            "{}",
            stderr(&out)
        );
    }
}

#[test]
fn tag_refusal_suggests_detach() {
    let s = Scratch::new();
    s.git(&["tag", "v1"]);
    let out = s.tonic(&["add", "v1"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("tonic add --detach v1"),
        "{}",
        stderr(&out)
    );
}
