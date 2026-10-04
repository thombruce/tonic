//! Bare-repo integration tests (#84) — the paths real-world bugs lived in
//! (#74 bare-dir base, #78 lineage, #81 rm cleanup) that the normal-repo harness
//! couldn't exercise. Worktrees land inside the bare dir; tonic is driven both
//! from a worktree and from the bare dir itself.
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

fn branches(s: &Scratch) -> Vec<String> {
    s.git_in(&s.bare_dir(), &["branch", "--format=%(refname:short)"])
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn bare_add_from_a_worktree_uses_the_current_branch() {
    let s = Scratch::bare();
    // from the main/ worktree (on main), a new branch roots on main — silently
    let out = s.tonic(&["add", "feat"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        s.wt("feat").exists(),
        "worktree not created in the bare dir"
    );
    assert!(
        !stderr(&out).contains("no base given") && !stderr(&out).contains("detached"),
        "a normal worktree add should not warn:\n{}",
        stderr(&out)
    );
}

#[test]
fn bare_add_from_the_bare_dir_roots_on_trunk_not_the_head_symref() {
    let s = Scratch::bare();
    // a branch with its own commit, and point the bare HEAD symref at it (a stale
    // symref unrelated to any checkout — the #74 trap)
    s.tonic(&["add", "other"]);
    s.commit_in(&s.wt("other"), "oc");
    s.git_in(&s.bare_dir(), &["symbolic-ref", "HEAD", "refs/heads/other"]);

    // add from the BARE DIR with no --base
    let out = s.tonic_in(&s.bare_dir(), &["add", "feat"]);
    assert!(out.status.success(), "{}", stderr(&out));
    // rooted on the trunk (main), not on `other` (the bare HEAD symref)
    let log = s.git_in(&s.wt("feat"), &["log", "--format=%s"]);
    assert!(
        !log.contains("oc"),
        "feat must not include other's commit:\n{log}"
    );
    assert!(
        stderr(&out).contains("starting from main"),
        "expected the bare-dir no-base warning:\n{}",
        stderr(&out)
    );
}

#[test]
fn bare_list_shows_stack_lineage() {
    let s = Scratch::bare();
    s.tonic(&["add", "a"]);
    s.commit_in(&s.wt("a"), "ac");
    s.tonic_in(&s.wt("a"), &["add", "b"]);
    s.commit_in(&s.wt("b"), "bc");
    let out = stdout(&s.tonic(&["list"]));
    assert!(
        out.contains("main → a → *b"),
        "bare stack lineage wrong:\n{out}"
    );
}

#[test]
fn bare_rm_auto_deletes_an_empty_branch_stacked_on_a_descendant() {
    let s = Scratch::bare();
    s.tonic(&["add", "a"]);
    s.commit_in(&s.wt("a"), "ac");
    s.tonic_in(&s.wt("a"), &["add", "b"]); // b off a, empty
    let out = s.tonic_in(&s.bare_dir(), &["rm", "b"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        !branches(&s).iter().any(|x| x == "b"),
        "empty branch should be auto-deleted in a bare repo:\n{:?}",
        branches(&s)
    );
}

#[test]
fn bare_rm_of_the_current_worktree_prints_an_escape_path() {
    let s = Scratch::bare();
    s.tonic(&["add", "feat"]);
    s.commit_in(&s.wt("feat"), "fc"); // real work, so it isn't auto-deleted — focus on the escape
    // remove feat from *inside* its own worktree: stdout must carry a path to cd
    // out to, which for a bare repo is the main/ worktree (home_checkout's bare arm)
    let out = s.tonic_in(&s.wt("feat"), &["rm", "feat", "-f"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let path = stdout(&out);
    assert!(
        !path.trim().is_empty(),
        "no escape path printed for a removed-current worktree"
    );
    assert!(
        path.trim().ends_with("main"),
        "escape path should be the main/ worktree:\n{path}"
    );
}

#[test]
fn bare_cd_resolves_from_the_bare_dir() {
    let s = Scratch::bare();
    s.tonic(&["add", "feat"]);
    // cd resolved from the bare dir prints the worktree path
    let out = s.tonic_in(&s.bare_dir(), &["cd", "feat"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).trim().ends_with("feat"),
        "cd from the bare dir should print feat's worktree:\n{}",
        stdout(&out)
    );
}
