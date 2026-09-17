//! Scratch-repo harness for the integration tests. Builds a real git repo in a
//! tempdir and drives the built `tonic` binary against it — automating the
//! by-hand verification the git-shelling code relied on (#50).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::string_slice,
    dead_code
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

pub struct Scratch {
    /// Kept so the tempdir isn't dropped for the test's lifetime.
    _tmp: TempDir,
    /// Parent of the repo — where sibling worktrees land.
    root: PathBuf,
    /// The main working tree.
    pub repo: PathBuf,
}

impl Scratch {
    /// A fresh repo on `main` with one commit (`m0`).
    pub fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        let s = Scratch { _tmp: tmp, root, repo };
        s.git(&["init", "-q", "-b", "main"]);
        s.git(&["config", "user.email", "t@t.co"]);
        s.git(&["config", "user.name", "t"]);
        s.commit_in(&s.repo, "m0");
        s
    }

    /// Run git in the main worktree, asserting success; returns trimmed stdout.
    pub fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.repo, args)
    }

    /// Run git in `dir`, asserting success; returns trimmed stdout.
    pub fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git").args(args).current_dir(dir).output().unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    /// Commit a unique file named `name` in `dir` (its content = `name`).
    pub fn commit_in(&self, dir: &Path, name: &str) {
        std::fs::write(dir.join(name), name).unwrap();
        self.git_in(dir, &["add", "."]);
        self.git_in(dir, &["commit", "-q", "-m", name]);
    }

    /// Run `tonic` in the main worktree.
    pub fn tonic(&self, args: &[&str]) -> Output {
        self.tonic_in(&self.repo, args)
    }

    /// Run `tonic` in `dir`.
    pub fn tonic_in(&self, dir: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tonic"))
            .args(args)
            .current_dir(dir)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }

    /// A path under the repo's parent dir (where siblings live).
    pub fn root_join(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// The path of the worktree `tonic add <branch>` creates (default template
    /// `{repo}-{branch}`, `/` flattened to `-`), a sibling of the main worktree.
    pub fn wt(&self, branch: &str) -> PathBuf {
        self.root.join(format!("repo-{}", branch.replace('/', "-")))
    }
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}
