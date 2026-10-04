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
    /// Parent of the repo — where sibling worktrees land (normal repos).
    root: PathBuf,
    /// The main working tree (normal), or the `main/` worktree (bare).
    pub repo: PathBuf,
    /// The bare dir, for a bare repo (`None` for a normal one). Worktrees land
    /// inside it (tonic's bare layout: base = the bare dir, template `{branch}`).
    bare_dir: Option<PathBuf>,
}

impl Scratch {
    /// A fresh repo on `main` with one commit (`m0`).
    pub fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        let s = Scratch {
            _tmp: tmp,
            root,
            repo,
            bare_dir: None,
        };
        s.git(&["init", "-q", "-b", "main"]);
        s.git(&["config", "user.email", "t@t.co"]);
        s.git(&["config", "user.name", "t"]);
        s.commit_in(&s.repo, "m0");
        s
    }

    /// A fresh **bare** repo (`barerepo.git`) on `main` with one commit and a
    /// `main/` worktree inside it. `repo` is that `main/` worktree; `bare_dir()`
    /// is the bare dir itself (run tonic there to exercise bare-dir invocation).
    /// Seeded by cloning a throwaway normal repo `--bare`, then detaching its
    /// origin so the bare repo stands alone (trunk resolves via `main` existing).
    pub fn bare() -> Self {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let bare = root.join("barerepo.git");
        // partially-built Scratch so the git_in/commit_in helpers are usable
        let mut s = Scratch {
            _tmp: tmp,
            root: root.clone(),
            repo: root.clone(),
            bare_dir: None,
        };
        // seed a normal repo with one commit, then bare-clone it
        let seed = root.join("seed");
        std::fs::create_dir(&seed).unwrap();
        s.git_in(&seed, &["init", "-q", "-b", "main"]);
        s.git_in(&seed, &["config", "user.email", "t@t.co"]);
        s.git_in(&seed, &["config", "user.name", "t"]);
        s.commit_in(&seed, "m0");
        s.git_in(&root, &["clone", "-q", "--bare", "seed", "barerepo.git"]);
        s.git_in(&bare, &["remote", "remove", "origin"]); // stand alone
        s.git_in(&bare, &["config", "user.email", "t@t.co"]);
        s.git_in(&bare, &["config", "user.name", "t"]);
        // the main worktree, inside the bare dir
        s.git_in(&bare, &["worktree", "add", "-q", "main", "main"]);
        s.repo = bare.join("main");
        s.bare_dir = Some(bare);
        s
    }

    /// The bare dir (panics if this isn't a bare `Scratch`).
    pub fn bare_dir(&self) -> PathBuf {
        self.bare_dir.clone().expect("not a bare Scratch")
    }

    /// Run git in the main worktree, asserting success; returns trimmed stdout.
    pub fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.repo, args)
    }

    /// Run git in `dir`, asserting success; returns trimmed stdout.
    pub fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        cmd.args(args).current_dir(dir);
        self.isolate(&mut cmd);
        let out = cmd.output().unwrap();
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
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tonic"));
        cmd.args(args).current_dir(dir).env("NO_COLOR", "1");
        self.isolate(&mut cmd);
        cmd.output().unwrap()
    }

    /// Point HOME / XDG_CONFIG_HOME at the tempdir so neither git's global config
    /// nor tonic's `~/.config/tonic/config.toml` (which could override
    /// `worktree_path` and break `wt()`) leaks in — the harness runs identically
    /// regardless of who runs it. (macOS ignores XDG, so HOME covers it there.)
    fn isolate(&self, cmd: &mut Command) {
        cmd.env("HOME", &self.root)
            .env("XDG_CONFIG_HOME", self.root.join(".config"));
    }

    /// A path under the repo's parent dir (where siblings live).
    pub fn root_join(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// The path of the worktree `tonic add <branch>` creates, `/` flattened to
    /// `-`: a `{repo}-{branch}` sibling for a normal repo, or `{branch}` inside the
    /// bare dir for a bare repo (tonic's bare layout).
    pub fn wt(&self, branch: &str) -> PathBuf {
        let flat = branch.replace('/', "-");
        match &self.bare_dir {
            Some(bare) => bare.join(flat),
            None => self.root.join(format!("repo-{flat}")),
        }
    }
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}
