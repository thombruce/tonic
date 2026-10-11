//! `tonic add --pr N` (#27): tonic makes the worktree, the forge CLI checks the
//! PR out inside it. Driven against a fake `gh` on PATH that logs its calls and
//! plays back a canned PR, so no network or real forge is involved.
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
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Output;

/// `pr view` prints $FAKE_PR_JSON; `pr checkout N --branch NAME` creates NAME at
/// the worktree's HEAD (or fails when $FAKE_GH_FAIL=1). Every call is logged.
const FAKE_GH: &str = r#"#!/bin/sh
echo "$*" >> "$FAKE_GH_LOG"
case "$1 $2" in
  "pr view") printf '%s\n' "$FAKE_PR_JSON" ;;
  "pr checkout")
    [ "$FAKE_GH_FAIL" = 1 ] && { echo "gh: checkout failed" >&2; exit 1; }
    git checkout -q -b "$5" ;;
esac
"#;

struct Forge {
    s: Scratch,
    bin: PathBuf,
    log: PathBuf,
}

impl Forge {
    /// A repo whose origin is on GitHub, with the fake `gh` ready.
    fn new() -> Self {
        Self::on(Scratch::new())
    }

    fn on(s: Scratch) -> Self {
        s.git(&["remote", "add", "origin", "https://github.com/o/r.git"]);
        let bin = s.root_join("fakebin");
        std::fs::create_dir(&bin).unwrap();
        let gh = bin.join("gh");
        std::fs::write(&gh, FAKE_GH).unwrap();
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let log = s.root_join("gh.log");
        Forge { s, bin, log }
    }

    fn json(head: &str, fork: bool) -> String {
        format!(r#"{{"headRefName":"{head}","isCrossRepository":{fork}}}"#)
    }

    fn run(&self, args: &[&str], json: &str, fail: bool) -> Output {
        self.run_in(&self.s.repo, args, json, fail)
    }

    fn run_in(&self, dir: &std::path::Path, args: &[&str], json: &str, fail: bool) -> Output {
        let path = format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap());
        self.s.tonic_env(
            dir,
            args,
            &[
                ("PATH", &path),
                ("FAKE_GH_LOG", self.log.to_str().unwrap()),
                ("FAKE_PR_JSON", json),
                ("FAKE_GH_FAIL", if fail { "1" } else { "0" }),
            ],
        )
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

#[test]
fn same_repo_pr_gets_a_pr_named_worktree_on_its_head_branch() {
    let f = Forge::new();
    let out = f.run(&["add", "--pr", "7"], &Forge::json("feat", false), false);
    assert!(out.status.success(), "{}", stderr(&out));
    let wt = f.s.wt("pr-7");
    assert_eq!(PathBuf::from(stdout(&out).trim()), wt);
    assert_eq!(f.s.git_in(&wt, &["branch", "--show-current"]), "feat");
    assert!(
        f.log().contains("pr checkout 7 --branch feat"),
        "{}",
        f.log()
    );
    // `tonic cd pr-7` finds it by the PR name
    let out = f.s.tonic(&["cd", "pr-7"]);
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn fork_pr_gets_a_pr_n_branch() {
    let f = Forge::new();
    // the fork's head is its own `main` — must not collide with ours
    let out = f.run(&["add", "--pr", "8"], &Forge::json("main", true), false);
    assert!(out.status.success(), "{}", stderr(&out));
    let wt = f.s.wt("pr-8");
    assert_eq!(f.s.git_in(&wt, &["branch", "--show-current"]), "pr-8");
    assert!(
        f.log().contains("pr checkout 8 --branch pr-8"),
        "{}",
        f.log()
    );
    assert_eq!(
        f.s.git(&["branch", "--show-current"]),
        "main",
        "ours untouched"
    );
}

#[test]
fn pr_already_checked_out_cds_there() {
    let f = Forge::new();
    f.s.tonic(&["add", "feat"]);
    let out = f.run(&["add", "--pr", "7"], &Forge::json("feat", false), false);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).trim().ends_with("repo-feat"),
        "{}",
        stdout(&out)
    );
    assert!(
        !f.log().contains("checkout"),
        "no checkout expected:\n{}",
        f.log()
    );
    assert!(!f.s.wt("pr-7").exists());
}

#[test]
fn failed_checkout_leaves_no_worktree() {
    let f = Forge::new();
    let out = f.run(&["add", "--pr", "7"], &Forge::json("feat", false), true);
    assert!(!out.status.success());
    assert!(!f.s.wt("pr-7").exists());
    assert!(!f.s.git(&["worktree", "list"]).contains("pr-7"));
}

#[test]
fn pr_worktree_path_config_names_the_folder() {
    let f = Forge::new();
    std::fs::write(
        f.s.repo.join("tonic.toml"),
        "pr_worktree_path = \"{repo}-{branch}-{pr}\"\n",
    )
    .unwrap();
    let out = f.run(&["add", "--pr", "7"], &Forge::json("feat/x", false), false);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(f.s.root_join("repo-feat-x-7").exists());
}

#[test]
fn non_github_origin_and_missing_cli_are_refused() {
    let f = Forge::new();
    f.s.git(&["remote", "set-url", "origin", "https://gitlab.com/o/r.git"]);
    let out = f.run(&["add", "--pr", "7"], &Forge::json("feat", false), false);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("supports GitHub"), "{}", stderr(&out));

    // GitHub origin but no gh on PATH: only git is reachable
    f.s.git(&["remote", "set-url", "origin", "https://github.com/o/r.git"]);
    let only_git = f.s.root_join("onlygit");
    std::fs::create_dir(&only_git).unwrap();
    let git = String::from_utf8(
        std::process::Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    std::os::unix::fs::symlink(git.trim(), only_git.join("git")).unwrap();
    let out = f.s.tonic_env(
        &f.s.repo,
        &["add", "--pr", "7"],
        &[("PATH", only_git.to_str().unwrap())],
    );
    assert!(!out.status.success());
    assert!(stderr(&out).contains("gh not found"), "{}", stderr(&out));
}

#[test]
fn pr_conflicts_with_a_branch_and_branch_flags() {
    let f = Forge::new();
    for args in [
        vec!["add", "--pr", "7", "feat"],
        vec!["add", "--pr", "7", "-b"],
        vec!["add", "--pr", "7", "--detach"],
    ] {
        let out = f.run(&args, &Forge::json("feat", false), false);
        assert!(!out.status.success(), "{args:?} should conflict");
        assert!(
            stderr(&out).contains("cannot be used with"),
            "{}",
            stderr(&out)
        );
    }
}

#[test]
fn rerun_finds_the_pr_folder_after_its_head_moved() {
    let f = Forge::new();
    let json = Forge::json("feat", false);
    f.run(&["add", "--pr", "7"], &json, false);
    let wt = f.s.wt("pr-7");
    f.s.git_in(&wt, &["checkout", "-q", "--detach"]); // e.g. after a rebase or `gh stack checkout`

    let out = f.run(&["add", "--pr", "7"], &json, false);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(PathBuf::from(stdout(&out).trim()), wt);
    assert_eq!(f.log().matches("pr checkout").count(), 1, "{}", f.log());
}

#[test]
fn missing_pr_folder_hint_names_a_command_that_works() {
    let f = Forge::new();
    let json = Forge::json("feat", true);
    f.run(&["add", "--pr", "7"], &json, false);
    let wt = f.s.wt("pr-7");
    f.s.git_in(&wt, &["checkout", "-q", "--detach"]); // off pr-7
    std::fs::remove_dir_all(&wt).unwrap();

    let out = f.run(&["add", "--pr", "7"], &json, false);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("tonic rm repo-pr-7"),
        "{}",
        stderr(&out)
    );
    // ...and that command actually cleans it up
    let out = f.s.tonic(&["rm", "repo-pr-7"]);
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn bare_dir_pr_does_not_depend_on_the_bare_head() {
    let f = Forge::on(Scratch::bare());
    let bare = f.s.bare_dir();
    // the bare repo's HEAD names a branch that doesn't exist (trunk is main)
    f.s.git_in(&bare, &["symbolic-ref", "HEAD", "refs/heads/master"]);
    let out = f.run_in(
        &bare,
        &["add", "--pr", "7"],
        &Forge::json("feat", false),
        false,
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        f.s.git_in(&f.s.wt("pr-7"), &["branch", "--show-current"]),
        "feat"
    );
}

#[test]
fn a_head_named_like_a_ref_falls_back_to_pr_n() {
    let f = Forge::new();
    // a same-repo head named like a tag: a branch `v2` would be ambiguous
    f.s.git(&["tag", "v2"]);
    let json = Forge::json("v2", false);
    let out = f.run(&["add", "--pr", "7"], &json, false);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        f.s.git_in(&f.s.wt("pr-7"), &["branch", "--show-current"]),
        "pr-7"
    );
    // a re-run falls back to the same pr-7 (now a local branch) and cd's there
    let again = f.run(&["add", "--pr", "7"], &json, false);
    assert!(again.status.success(), "{}", stderr(&again));
    assert_eq!(stdout(&again), stdout(&out));
}
