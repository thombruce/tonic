use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct Config {
    /// Path template for new worktrees, resolved relative to the repo's parent
    /// dir. Placeholders: {repo}, {branch}. Default: "{repo}.git/{branch}".
    worktree_path: Option<String>,
    /// Per-pattern mode overrides for entries listed in `.worktreeinclude`.
    include: Option<Vec<Include>>,
    hooks: Option<Vec<Hook>>,
}

#[derive(Deserialize, Clone)]
struct Include {
    pattern: String,
    #[serde(default)]
    mode: Mode,
}

#[derive(Deserialize, Clone, Copy, Default, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
enum Mode {
    #[default]
    Copy,
    Symlink,
}

#[derive(Deserialize, Clone)]
struct Hook {
    event: String,
    run: String,
}

impl Config {
    /// Load and merge the config chain, lowest priority first. Later files win
    /// per key, mirroring git's system < global < local precedence:
    ///   1. ~/.config/tonic/config.toml   (global defaults)
    ///   2. <repo>/tonic.toml             (shared, committed — later)
    ///   3. <gitdir>/tonic.toml           (per-repo, private — highest)
    fn load(repo: &Repo) -> Result<Config> {
        let mut cfg = Config::default();
        let global = dirs::config_dir().map(|d| d.join("tonic/config.toml"));
        let chain = [
            global,
            repo.root.as_ref().map(|r| r.join("tonic.toml")),
            Some(repo.git_dir.join("tonic.toml")),
        ];
        for path in chain.into_iter().flatten() {
            if path.exists() {
                let text = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let next: Config = toml::from_str(&text)
                    .with_context(|| format!("parsing {}", path.display()))?;
                cfg.merge(next);
            }
        }
        Ok(cfg)
    }

    fn merge(&mut self, higher: Config) {
        if higher.worktree_path.is_some() {
            self.worktree_path = higher.worktree_path;
        }
        if higher.include.is_some() {
            self.include = higher.include;
        }
        if higher.hooks.is_some() {
            self.hooks = higher.hooks;
        }
    }

    fn mode_for(&self, pattern: &str) -> Mode {
        self.include
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .find(|i| i.pattern == pattern)
            .map(|i| i.mode)
            .unwrap_or(Mode::Copy)
    }
}

// ---------------------------------------------------------------------------
// Repo discovery
// ---------------------------------------------------------------------------

struct Repo {
    /// Main working-tree root. `None` for a bare repo (no working tree).
    root: Option<PathBuf>,
    /// The common git dir: `.git` for a normal repo, the bare dir itself for a
    /// bare repo.
    git_dir: PathBuf,
    name: String,
    bare: bool,
}

impl Repo {
    fn discover() -> Result<Repo> {
        let bare = git_capture(None, &["rev-parse", "--is-bare-repository"])? == "true";
        // --show-toplevel errors in a bare repo, so only ask when not bare.
        let root = if bare {
            None
        } else {
            let toplevel = git_capture(None, &["rev-parse", "--show-toplevel"])?;
            (!toplevel.is_empty()).then(|| PathBuf::from(toplevel))
        };
        // --git-common-dir is relative to the dir git ran in.
        let here = root.clone().map(Ok).unwrap_or_else(std::env::current_dir)?;
        let common = git_capture(Some(&here), &["rev-parse", "--git-common-dir"])?;
        let git_dir = abs(&here, Path::new(&common));
        // --git-common-dir is often ".", leaving "/./" in derived paths; clean it.
        let git_dir = std::fs::canonicalize(&git_dir).unwrap_or(git_dir);
        // Name from the working tree, else the bare dir with a trailing ".git"
        // stripped (barerepo.git -> barerepo).
        let dir = root.as_deref().unwrap_or(&git_dir);
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().trim_end_matches(".git").to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "repo".into());
        Ok(Repo { root, git_dir, name, bare })
    }

    /// Directory to run git subcommands from.
    fn cwd(&self) -> &Path {
        self.root.as_deref().unwrap_or(&self.git_dir)
    }

}

/// Directory to source `.worktreeinclude` entries from. For a normal repo (or
/// when invoked inside a worktree) that's the current working tree. For a bare
/// repo invoked from the bare dir there is no working tree, so fall back to the
/// main/ then master/ worktree; `None` if neither exists yet.
fn resolve_source(repo: &Repo) -> Result<Option<PathBuf>> {
    if let Some(root) = &repo.root {
        return Ok(Some(root.clone()));
    }
    let list = git_capture(Some(repo.cwd()), &["worktree", "list", "--porcelain"])?;
    for branch in ["main", "master"] {
        if let Some(path) = parse_worktree(&list, branch) {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// True if `rel` (relative to `dir`) is gitignored.
fn is_ignored(dir: &Path, rel: &Path) -> bool {
    // check-ignore exits 0 when ignored, 1 when not — can't use git_run.
    Command::new("git")
        .args(["check-ignore", "--quiet"])
        .arg(rel)
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// True if `branch` exists as a local branch.
fn branch_exists(repo: &Repo, branch: &str) -> bool {
    Command::new("git")
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .current_dir(repo.cwd())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Base dir that a `worktree_path` template resolves against: the parent of the
/// working tree for a normal repo, or the bare dir itself for a bare repo (so
/// worktrees land as siblings inside `barerepo.git/`).
fn worktree_base(root: Option<&Path>, git_dir: &Path) -> PathBuf {
    match root {
        Some(r) => r.parent().unwrap_or(r).to_path_buf(),
        None => git_dir.to_path_buf(),
    }
}

// ---------------------------------------------------------------------------
// add
// ---------------------------------------------------------------------------

pub fn add(branch: &str, new_branch: bool) -> Result<()> {
    let repo = Repo::discover()?;
    let cfg = Config::load(&repo)?;

    let default_template = if repo.bare { "{branch}" } else { "{repo}.git/{branch}" };
    let template = cfg.worktree_path.as_deref().unwrap_or(default_template);
    let rendered = render(template, &[("repo", &repo.name), ("branch", branch)]);
    let path = {
        let p = PathBuf::from(&rendered);
        if p.is_absolute() {
            p
        } else {
            worktree_base(repo.root.as_deref(), &repo.git_dir).join(p)
        }
    };
    if path.exists() {
        bail!("worktree path already exists: {}", path.display());
    }

    let path_str = path.to_string_lossy().into_owned();
    // `git worktree add <path> <branch>` requires <branch> to be an existing
    // ref. If it doesn't exist locally, create it (as `-b` would) so plain
    // `tonic add foo` works whether foo is new or existing. ponytail: a missing
    // local branch is created from HEAD; it won't auto-track a remote branch of
    // the same name — add remote DWIM (`--guess-remote`) if that's wanted.
    let create = new_branch || !branch_exists(&repo, branch);
    let mut args = vec!["worktree", "add"];
    if create {
        args.extend(["-b", branch, &path_str]);
    } else {
        args.extend([&path_str, branch]);
    }
    git_run(repo.cwd(), &args)?;

    transfer_includes(&repo, &path, &cfg)?;
    run_hooks(&cfg, "post_create", &repo, branch, &path)?;

    println!("worktree ready: {}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// rm
// ---------------------------------------------------------------------------

pub fn rm(branch: &str, force: bool, delete_branch: bool) -> Result<()> {
    let repo = Repo::discover()?;
    let cfg = Config::load(&repo)?;

    let list = git_capture(Some(repo.cwd()), &["worktree", "list", "--porcelain"])?;
    let path = parse_worktree(&list, branch)
        .ok_or_else(|| anyhow!("no worktree checked out for branch '{branch}'"))?;

    run_hooks(&cfg, "pre_remove", &repo, branch, &path)?;

    let path_str = path.to_string_lossy().into_owned();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&path_str);
    git_run(repo.cwd(), &args)?;

    if delete_branch {
        // -d refuses unmerged branches; -f opts into -D's force delete.
        let flag = if force { "-D" } else { "-d" };
        git_run(repo.cwd(), &["branch", flag, branch])?;
    }

    println!("removed worktree: {}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

pub fn list() -> Result<()> {
    let repo = Repo::discover()?;
    let out = git_capture(Some(repo.cwd()), &["worktree", "list", "--porcelain"])?;
    for w in parse_worktrees(&out) {
        println!("{}  [{}]", w.path.display(), w.label);
    }
    Ok(())
}

struct Worktree {
    path: PathBuf,
    /// Branch name, or "detached" / "bare".
    label: String,
}

/// Parse `git worktree list --porcelain`. Each block is a `worktree <path>`
/// line plus `branch refs/heads/<name>` (or a bare `detached`/`bare` line).
fn parse_worktrees(porcelain: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut label = String::from("detached");
    for line in porcelain.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            if let Some(prev) = path.take() {
                out.push(Worktree { path: prev, label: std::mem::replace(&mut label, "detached".into()) });
            }
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            label = b.to_string();
        } else if line == "bare" || line == "detached" {
            label = line.to_string();
        }
    }
    if let Some(p) = path {
        out.push(Worktree { path: p, label });
    }
    out
}

/// Path of the worktree checked out for `branch`, if any.
fn parse_worktree(porcelain: &str, branch: &str) -> Option<PathBuf> {
    parse_worktrees(porcelain)
        .into_iter()
        .find(|w| w.label == branch)
        .map(|w| w.path)
}

fn transfer_includes(repo: &Repo, worktree: &Path, cfg: &Config) -> Result<()> {
    let Some(source) = resolve_source(repo)? else {
        return Ok(());
    };
    let list = source.join(".worktreeinclude");
    if !list.exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&list)?;
    for line in text.lines() {
        let pattern = line.trim();
        if pattern.is_empty() || pattern.starts_with('#') {
            continue;
        }
        // ponytail: glob is top-level/relative, not full gitignore semantics
        // (e.g. bare `node_modules` won't match at every depth). It also globs
        // the whole absolute path, so a `[ * ?` metacharacter in a *parent* dir
        // name mis-parses and matches nothing. Swap in the `ignore` crate if
        // either ceiling bites.
        let full = source.join(pattern);
        let mode = cfg.mode_for(pattern);
        // A malformed pattern shouldn't abort mid-add after the worktree exists;
        // warn and skip that line instead.
        let entries = match glob::glob(&full.to_string_lossy()) {
            Ok(entries) => entries,
            Err(err) => {
                eprintln!("warning: skipping invalid .worktreeinclude pattern '{pattern}': {err}");
                continue;
            }
        };
        for entry in entries.flatten() {
            let rel = entry.strip_prefix(&source).unwrap_or(&entry);
            // Only transfer gitignored files — never fork a tracked/committed
            // file into the worktree. Matches Claude/worktrunk's guardrail.
            if !is_ignored(&source, rel) {
                continue;
            }
            let dest = worktree.join(rel);
            transfer(&entry, &dest, mode)
                .with_context(|| format!("transferring {}", rel.display()))?;
        }
    }
    Ok(())
}

fn transfer(src: &Path, dest: &Path, mode: Mode) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match mode {
        Mode::Symlink => {
            let target = std::fs::canonicalize(src)?;
            // ponytail: unix-only. Add windows symlink_dir/symlink_file if asked.
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, dest)?;
            #[cfg(not(unix))]
            bail!("symlink mode is unix-only for now");
        }
        Mode::Copy => copy_recursive(src, dest)?,
    }
    Ok(())
}

fn copy_recursive(src: &Path, dest: &Path) -> Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dest.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(src, dest)?;
    }
    Ok(())
}

fn run_hooks(cfg: &Config, event: &str, repo: &Repo, branch: &str, worktree: &Path) -> Result<()> {
    let wt = worktree.to_string_lossy();
    let vars = [("repo", repo.name.as_str()), ("branch", branch), ("worktree_path", &wt)];
    for hook in cfg.hooks.as_deref().unwrap_or(&[]).iter().filter(|h| h.event == event) {
        let cmd = render(&hook.run, &vars);
        println!("[{event}] {cmd}");
        let status = Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(worktree)
            .status()
            .with_context(|| format!("running hook: {cmd}"))?;
        if !status.success() {
            bail!("hook failed ({event}): {cmd}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn render(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

fn abs(base: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

fn git_run(dir: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .context("running git")?;
    if !status.success() {
        bail!("git {} failed", args.join(" "));
    }
    Ok(())
}

fn git_capture(dir: Option<&Path>, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let out = cmd.output().context("running git")?;
    if !out.status.success() {
        bail!("git {} failed", args.join(" "));
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_substitutes() {
        let out = render("{repo}.git/{branch}", &[("repo", "tonic"), ("branch", "feat/x")]);
        assert_eq!(out, "tonic.git/feat/x");
    }

    #[test]
    fn merge_later_wins_per_key() {
        let mut base = Config {
            worktree_path: Some("a".into()),
            include: None,
            hooks: None,
        };
        base.merge(Config {
            worktree_path: Some("b".into()),
            include: None,
            hooks: None,
        });
        assert_eq!(base.worktree_path.as_deref(), Some("b"));
    }

    #[test]
    fn worktree_base_normal_vs_bare() {
        // normal: base is the parent of the working tree
        assert_eq!(
            worktree_base(Some(Path::new("/src/myrepo")), Path::new("/src/myrepo/.git")),
            PathBuf::from("/src")
        );
        // bare: base is the bare dir itself (siblings land inside it)
        assert_eq!(
            worktree_base(None, Path::new("/src/myrepo.git")),
            PathBuf::from("/src/myrepo.git")
        );
    }

    #[test]
    fn parse_worktree_finds_branch() {
        let out = "worktree /repo/main\nHEAD abc\nbranch refs/heads/main\n\n\
                   worktree /repo/.worktrees/feat\nHEAD def\nbranch refs/heads/feat\n";
        assert_eq!(parse_worktree(out, "feat"), Some(PathBuf::from("/repo/.worktrees/feat")));
        assert_eq!(parse_worktree(out, "nope"), None);
    }

    #[test]
    fn mode_defaults_to_copy() {
        let cfg = Config {
            worktree_path: None,
            include: Some(vec![Include { pattern: "node_modules".into(), mode: Mode::Symlink }]),
            hooks: None,
        };
        assert_eq!(cfg.mode_for("node_modules"), Mode::Symlink);
        assert_eq!(cfg.mode_for(".env"), Mode::Copy);
    }
}
