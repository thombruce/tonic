use anyhow::{anyhow, bail, Context, Result};
use owo_colors::{OwoColorize, Stream};
use serde::Deserialize;
use std::collections::HashMap;
#[cfg(unix)]
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct Config {
    /// Path template for new worktrees, resolved relative to the repo's parent
    /// dir. Placeholders: {repo}, {branch}. Default: "{repo}-{branch}" (normal
    /// repo) or "{branch}" (bare).
    worktree_path: Option<String>,
    /// Default start-point (any ref) for a *new* branch when `--base` isn't
    /// given; with neither, a new branch is created from HEAD. No effect when
    /// checking out an existing local or remote branch.
    base: Option<String>,
    /// Fetch before resolving a non-local branch, so a branch that exists on a
    /// remote but hasn't been fetched is picked up. Mirrors the `--fetch` flag.
    fetch: Option<bool>,
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
        if higher.base.is_some() {
            self.base = higher.base;
        }
        if higher.fetch.is_some() {
            self.fetch = higher.fetch;
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
    /// The working tree tonic was invoked in. `None` when invoked from a bare
    /// dir (no working tree). Used as the `.worktreeinclude` source and the dir
    /// to run git from — not for identity, which must not vary by worktree.
    root: Option<PathBuf>,
    /// The common git dir: `.git` for a normal repo, the bare dir itself for a
    /// bare repo.
    git_dir: PathBuf,
    /// The repository's main worktree (or the bare dir if bare). Identity and
    /// worktree layout derive from this, so they're the same from any worktree.
    main: PathBuf,
    name: String,
    bare: bool,
}

impl Repo {
    fn discover() -> Result<Repo> {
        // Whether the *current dir* has no working tree (a bare dir): needed
        // only because --show-toplevel errors there. Not the same as the repo
        // being bare — a linked worktree of a bare repo has a working tree.
        // First git call — a failure here almost always means we're not in a repo.
        let no_worktree = git_capture(None, &["rev-parse", "--is-bare-repository"])
            .context("not a git repository")?
            == "true";
        let root = if no_worktree {
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

        // `core.bare` in the shared config is the authoritative bare flag and is
        // independent of cwd — unlike `--is-bare-repository`, which is false
        // inside a linked worktree of a bare repo. Reading it also avoids
        // misclassifying --separate-git-dir / submodule repos as bare.
        let bare = is_bare(&git_dir);

        // Identity and layout derive from the *repository's main worktree*, not
        // the invoking working tree — otherwise running from a linked worktree
        // named after its branch (e.g. bare.git/main) would use "main" as the
        // name. git lists the main worktree first; for a bare repo that entry is
        // the bare dir. (Under --separate-git-dir git reports the git dir here
        // rather than the checkout, so the name is best-effort in that setup;
        // bare-ness is still correct via core.bare above.)
        let list = git_capture(Some(&here), &["worktree", "list", "--porcelain"])?;
        let main = parse_worktrees(&list)
            .into_iter()
            .next()
            .map(|w| w.path)
            .unwrap_or_else(|| git_dir.clone());
        let name = main
            .file_name()
            .map(|n| {
                let n = n.to_string_lossy();
                // strip the bare dir's ".git" suffix (barerepo.git -> barerepo)
                if bare { n.trim_end_matches(".git").to_string() } else { n.into_owned() }
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "repo".into());
        Ok(Repo { root, git_dir, main, name, bare })
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

/// Append the git args for creating a new branch from HEAD (or from `base`).
fn new_branch_args(args: &mut Vec<String>, branch: &str, path: &str, base: Option<&str>) {
    args.extend(["-b".into(), branch.into(), path.to_string()]);
    if let Some(b) = base {
        args.push(b.to_string());
    }
}

/// Remotes that have a remote-tracking ref for `branch` (requires a prior fetch;
/// tonic matches existing `refs/remotes/*` refs, it does not fetch).
fn remotes_with_branch(repo: &Repo, branch: &str) -> Vec<String> {
    let remotes = git_capture(Some(repo.cwd()), &["remote"]).unwrap_or_default();
    remotes
        .lines()
        .filter(|r| {
            Command::new("git")
                .args(["show-ref", "--verify", "--quiet"])
                .arg(format!("refs/remotes/{r}/{branch}"))
                .current_dir(repo.cwd())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        })
        .map(|s| s.to_string())
        .collect()
}

/// Pick the remote to track `branch` from when it has no local branch. `explicit`
/// forces a remote (error if it lacks the branch). Otherwise prefer `origin`,
/// else the sole remote that has it; `None` if none do, error if ambiguous.
fn choose_remote(repo: &Repo, branch: &str, explicit: Option<&str>) -> Result<Option<String>> {
    let have = remotes_with_branch(repo, branch);
    if let Some(r) = explicit {
        return if have.iter().any(|x| x == r) {
            Ok(Some(r.to_string()))
        } else {
            bail!("remote '{r}' has no branch '{branch}'")
        };
    }
    if have.iter().any(|r| r == "origin") {
        return Ok(Some("origin".into()));
    }
    match have.len() {
        0 => Ok(None),
        1 => Ok(have.into_iter().next()),
        _ => bail!(
            "branch '{branch}' exists on multiple remotes: {}. use --remote <name>",
            have.join(", ")
        ),
    }
}

/// The repository's `core.bare` flag, read from the shared config so it doesn't
/// depend on cwd (a linked worktree of a bare repo reads the same value).
fn is_bare(git_dir: &Path) -> bool {
    Command::new("git")
        .arg("config")
        .arg("--file")
        .arg(git_dir.join("config"))
        .args(["--get", "core.bare"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false)
}

/// Base dir that a `worktree_path` template resolves against: the parent of the
/// main working tree for a normal repo, or the bare dir itself for a bare repo
/// (so worktrees land as siblings inside `barerepo.git/`). `main` is the repo's
/// main worktree, so this is the same regardless of which worktree tonic runs in.
fn worktree_base(bare: bool, main: &Path) -> PathBuf {
    if bare {
        main.to_path_buf()
    } else {
        main.parent().unwrap_or(main).to_path_buf()
    }
}

/// The path where `add` places (and `rm`/`cd` look for) the worktree for
/// `branch`: the `worktree_path` template rendered and resolved against
/// `worktree_base`. `/` in the branch name is flattened to `-` so the worktree
/// is one directory, not nested subdirs (git/hooks still get the real name).
fn worktree_path_for(repo: &Repo, cfg: &Config, branch: &str) -> PathBuf {
    let default_template = if repo.bare { "{branch}" } else { "{repo}-{branch}" };
    let template = cfg.worktree_path.as_deref().unwrap_or(default_template);
    let path_branch = branch.replace('/', "-");
    let rendered = render(template, &[("repo", &repo.name), ("branch", &path_branch)]);
    let p = PathBuf::from(&rendered);
    if p.is_absolute() {
        p
    } else {
        worktree_base(repo.bare, &repo.main).join(p)
    }
}

/// Resolve `name` to an existing worktree path for `rm`/`cd`. A worktree is a
/// directory, so we don't rely on its current branch alone (it may be detached
/// or switched, e.g. after `gh stack checkout`). Match, in order: the branch
/// currently checked out; the path `add` would use for `name`; the worktree's
/// directory basename.
fn resolve_worktree(repo: &Repo, cfg: &Config, name: &str) -> Result<PathBuf> {
    let list = git_capture(Some(repo.cwd()), &["worktree", "list", "--porcelain"])?;
    let worktrees = parse_worktrees(&list);

    // 1. currently checked out on branch `name`.
    if let Some(w) = worktrees.iter().find(|w| !w.bare && w.label == name) {
        return Ok(w.path.clone());
    }
    // 2. the path add would create for `name` (handles detached/switched HEAD).
    // Canonicalize both sides the same way (falling back to raw) so a symlinked
    // base like macOS /tmp -> /private/tmp doesn't cause a spurious miss.
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let expected = canon(&worktree_path_for(repo, cfg, name));
    if let Some(w) = worktrees.iter().find(|w| !w.bare && canon(&w.path) == expected) {
        return Ok(w.path.clone());
    }
    // 3. the worktree's directory basename (handles template drift / rm by dir).
    if let Some(w) = worktrees
        .iter()
        .find(|w| !w.bare && w.path.file_name().and_then(|n| n.to_str()) == Some(name))
    {
        return Ok(w.path.clone());
    }
    bail!("no worktree found for '{name}'")
}

// ---------------------------------------------------------------------------
// add
// ---------------------------------------------------------------------------

pub fn add(
    branch: &str,
    new_branch: bool,
    base: Option<&str>,
    remote: Option<&str>,
    fetch: bool,
) -> Result<()> {
    let repo = Repo::discover()?;
    let cfg = Config::load(&repo)?;
    let fetch = fetch || cfg.fetch.unwrap_or(false);

    let path = worktree_path_for(&repo, &cfg, branch);
    if path.exists() {
        bail!("worktree path already exists: {}", path.display());
    }

    // `-b` and `--base` both mean "make a new branch" (from HEAD, or from base),
    // skipping the remote-tracking DWIM below.
    let force_new = new_branch || base.is_some();
    let local = branch_exists(&repo, branch);

    // `--remote` only has meaning on the remote-tracking path; if it can't apply,
    // say so rather than silently ignoring the user's explicit choice.
    if remote.is_some() {
        if force_new {
            bail!("--remote can't be combined with -b/--base, which create a new branch rather than track a remote");
        }
        if local {
            bail!("branch '{branch}' already exists locally; --remote only applies when creating a new branch from a remote");
        }
    }

    // A branch that already exists can't be re-created or checked out twice —
    // catch both cases with a clear message instead of git's raw error.
    if local {
        if force_new {
            bail!("branch '{branch}' already exists\n       omit -b/--base to check it out, or choose a new name");
        }
        // #16: a branch can only be checked out in one worktree.
        let list = git_capture(Some(repo.cwd()), &["worktree", "list", "--porcelain"])?;
        if let Some(p) = parse_worktree(&list, branch) {
            bail!(
                "branch '{branch}' is already checked out at {}\n       try: tonic cd {branch}",
                p.display()
            );
        }
    }

    // Start point for a new branch: --base flag, else the config `base` default.
    // The config only supplies the start point; it never forces a new branch
    // (checking out an existing/remote branch above is unaffected).
    let effective_base = base.or(cfg.base.as_deref());

    let path_str = path.to_string_lossy().into_owned();
    let mut args: Vec<String> = vec!["worktree".into(), "add".into()];
    let mut tracking: Option<String> = None;

    if !force_new && local {
        // Existing local branch, not checked out anywhere: check it out.
        args.push(path_str.clone());
        args.push(branch.into());
    } else if !force_new {
        // No local branch: check remotes. #15 — if exactly one remote (origin
        // preferred) has it, create a local branch tracking it. Otherwise fall
        // through to a new branch from HEAD.
        // #18: optionally fetch first so a branch that exists on a remote but
        // hasn't been fetched is picked up. Non-fatal — offline still resolves
        // against existing refs.
        if fetch {
            let mut fargs = vec!["fetch", "--quiet"];
            match remote {
                Some(r) => fargs.push(r),
                None => fargs.push("--all"),
            }
            if git_run(repo.cwd(), &fargs).is_err() {
                eprintln!("warning: fetch failed; resolving with existing refs");
            }
        }
        match choose_remote(&repo, branch, remote)? {
            Some(r) => {
                args.extend(["--track".into(), "-b".into(), branch.into(), path_str.clone()]);
                args.push(format!("{r}/{branch}"));
                tracking = Some(format!("{r}/{branch}"));
            }
            None => new_branch_args(&mut args, branch, &path_str, effective_base),
        }
    } else {
        // -b / --base: force a new branch.
        new_branch_args(&mut args, branch, &path_str, effective_base);
    }

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    git_run(repo.cwd(), &arg_refs)?;

    transfer_includes(&repo, &path, &cfg)?;
    run_hooks(&cfg, "post_create", &repo, branch, &path)?;

    // Human status → stderr; the worktree path → stdout only, so the shell
    // integration (`tonic shell-init`) can `cd "$(tonic add ...)"`.
    eprintln!(
        "{} created worktree for {branch}",
        "✓".if_supports_color(Stream::Stderr, |t| t.green())
    );
    if let Some(t) = tracking {
        eprintln!("  tracking {t}");
    }
    println!("{}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// rm
// ---------------------------------------------------------------------------

pub fn rm(branch: &str, force: bool, delete_branch: bool) -> Result<()> {
    let repo = Repo::discover()?;
    let cfg = Config::load(&repo)?;

    let path = resolve_worktree(&repo, &cfg, branch)?;

    // Are we standing in the worktree we're about to remove? (compare the
    // invoking working tree to the target). Computed before removal, since the
    // dir won't be canonicalizable afterwards.
    let removing_current = repo
        .root
        .as_deref()
        .and_then(|r| std::fs::canonicalize(r).ok())
        .zip(std::fs::canonicalize(&path).ok())
        .is_some_and(|(here, target)| here == target);

    run_hooks(&cfg, "pre_remove", &repo, branch, &path)?;

    // Run git from the main worktree, not the target: if we're removing the
    // current worktree, its dir is about to disappear from under us.
    let path_str = path.to_string_lossy().into_owned();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&path_str);
    git_run(&repo.main, &args)?;

    if delete_branch {
        // -d refuses unmerged branches; -f opts into -D's force delete.
        let flag = if force { "-D" } else { "-d" };
        git_run(&repo.main, &["branch", flag, branch])?;
    }

    eprintln!(
        "{} removed worktree: {}",
        "✓".if_supports_color(Stream::Stderr, |t| t.green()),
        path.display()
    );
    // If we just deleted the worktree we were in, print a safe fallback path on
    // stdout so the shell integration can `cd` there instead of leaving the
    // shell stranded in a deleted directory (#29).
    if removing_current {
        println!("{}", home_checkout(&repo).display());
    }
    Ok(())
}

/// A worktree to return to after removing the current one: the main working tree
/// for a normal repo; for a bare repo the `main`/`master` (or any remaining)
/// worktree, since `repo.main` there is the bare dir, which has no checkout (#35).
fn home_checkout(repo: &Repo) -> PathBuf {
    if !repo.bare {
        return repo.main.clone();
    }
    // Run from repo.main (the bare dir, which exists) — the current worktree we
    // just removed may be gone.
    if let Ok(list) = git_capture(Some(&repo.main), &["worktree", "list", "--porcelain"]) {
        for branch in ["main", "master"] {
            if let Some(p) = parse_worktree(&list, branch) {
                return p;
            }
        }
        if let Some(w) = parse_worktrees(&list).into_iter().find(|w| !w.bare) {
            return w.path;
        }
    }
    repo.main.clone()
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

pub fn list() -> Result<()> {
    let repo = Repo::discover()?;
    let out = git_capture(Some(repo.cwd()), &["worktree", "list", "--porcelain"])?;
    let worktrees = parse_worktrees(&out);

    // The current worktree is the one whose path is the invoking working tree.
    let current = repo.root.as_deref().and_then(|r| std::fs::canonicalize(r).ok());
    let width = worktrees.iter().map(|w| w.label.chars().count()).max().unwrap_or(0);
    // Branch tips + trunk drive lineage inference (#37); the annotation can name
    // stacked ancestors that have no worktree of their own.
    let tips = branch_tips(&repo);
    let default = default_branch(&tips);

    for w in &worktrees {
        let is_bare = w.bare;
        // The bare entry is the anchor, not an actionable worktree: dim it, and
        // it's never "current" (that belongs to an actual checkout).
        let is_current =
            !is_bare && current.is_some() && std::fs::canonicalize(&w.path).ok() == current;
        let dirty = !is_bare && is_dirty(&w.path);

        let label = format!("{:<width$}", w.label);
        let path = w.path.display().to_string();

        if is_bare {
            println!(
                "  {}  {}",
                label.if_supports_color(Stream::Stdout, |t| t.dimmed()),
                path.if_supports_color(Stream::Stdout, |t| t.dimmed()),
            );
            continue;
        }

        let marker = if is_current {
            format!("{}", "*".if_supports_color(Stream::Stdout, |t| t.green()))
        } else {
            " ".to_string()
        };
        let label = if is_current {
            let style = owo_colors::Style::new().green().bold();
            format!("{}", label.if_supports_color(Stream::Stdout, |t| t.style(style)))
        } else {
            label
        };
        let path = format!("{}", path.if_supports_color(Stream::Stdout, |t| t.dimmed()));
        let dirty = if dirty {
            format!(" {}", "(dirty)".if_supports_color(Stream::Stdout, |t| t.yellow()))
        } else {
            String::new()
        };
        // Stack lineage: only annotate a genuine stack (branch sits on another
        // branch, not just the trunk) — a chain of `root → … → branch`, dimmed.
        let chain = default.map(|d| lineage(&repo, &w.label, &tips, d)).unwrap_or_default();
        let stack = if chain.len() >= 3 {
            let s = format!("  {}", chain.join(" → "));
            format!("{}", s.if_supports_color(Stream::Stdout, |t| t.dimmed()))
        } else {
            String::new()
        };
        println!("{marker} {label}  {path}{dirty}{stack}");
    }
    Ok(())
}

/// Map of commit sha → local branch name(s) whose tip is that commit. One
/// `for-each-ref` — the basis for cheap lineage (no per-pair git calls).
fn branch_tips(repo: &Repo) -> HashMap<String, Vec<String>> {
    let out = git_capture(
        Some(repo.cwd()),
        &["for-each-ref", "--format=%(objectname) %(refname:short)", "refs/heads/"],
    )
    .unwrap_or_default();
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for line in out.lines() {
        if let Some((sha, name)) = line.split_once(' ') {
            map.entry(sha.to_string()).or_default().push(name.to_string());
        }
    }
    map
}

/// The repo's trunk among the local branches: `main`, else `master`.
/// ponytail: hardcoded pair; a configurable default branch is tracked in #46.
fn default_branch(tips: &HashMap<String, Vec<String>>) -> Option<&'static str> {
    let names: Vec<&str> = tips.values().flatten().map(String::as_str).collect();
    ["main", "master"].into_iter().find(|d| names.contains(d))
}

/// True if `a` and `b` share a common ancestor (a merge-base exists). Bounds the
/// lineage walk: with a common ancestor, `default..branch` is limited to the
/// divergent commits; unrelated roots would otherwise span all of branch.
fn share_history(repo: &Repo, a: &str, b: &str) -> bool {
    git_capture(Some(repo.cwd()), &["merge-base", a, b]).is_ok_and(|s| !s.is_empty())
}

/// The stack lineage for `branch` as `root → … → branch`, read from a single
/// first-parent walk of the range `default..branch` (so cost is the stack
/// height, not the repo's history). Intermediate branch tips on that walk are
/// the stack's lower branches. A chain of length < 3 means "not a stack" (a
/// branch directly on the trunk yields `[default, branch]`) → no annotation.
///
/// `--boundary` includes the fork commit at the bottom of the range, so a base
/// branch whose tip has been absorbed into `default` (main drifted/merged past
/// it) is still named — it wouldn't be with a plain `^default` exclusion.
///
/// `branch`'s own tip is skipped, so an empty branch sitting at its base's
/// commit yields `[default, branch]` — no lineage until it has a commit of its
/// own (transient, self-healing). ~2 git calls total, independent of the number
/// of branches and of history depth (contrast the old per-pair scan).
///
/// Assumes linear stacks (`--first-parent`): an ancestor branch reachable only
/// through a merge's second parent won't be named — matches the stated scope.
fn lineage(repo: &Repo, branch: &str, tips: &HashMap<String, Vec<String>>, default: &str) -> Vec<String> {
    // Guard: the branch must share history with the trunk. This is the cost
    // bound (without a common ancestor, `default..branch` spans all of branch),
    // and it admits a branch that has *diverged* from the trunk — a base whose
    // fork is now behind an advanced main is still a stack, unlike a plain
    // "is main an ancestor" test which would wrongly reject it.
    if branch == default || !share_history(repo, default, branch) {
        return vec![branch.to_string()];
    }
    let revs = git_capture(
        Some(repo.cwd()),
        &["rev-list", "--first-parent", "--boundary", &format!("{default}..{branch}")],
    )
    .unwrap_or_default();
    let mut chain = vec![branch.to_string()];
    // skip(1): the first rev is branch's own tip; ancestors are strictly below.
    for line in revs.lines().skip(1) {
        // boundary commits (the fork) are prefixed with '-'.
        let sha = line.strip_prefix('-').unwrap_or(line);
        let Some(names) = tips.get(sha) else {
            continue;
        };
        // Skip default's own tip commit: branches sitting there — empty branches
        // off the trunk, or a base that has landed in the trunk — are siblings of
        // the stack, not ancestors. (This is why the boundary can't be taken at
        // face value: it's the fork, where such branches cluster.)
        if names.iter().any(|n| n == default) {
            continue;
        }
        for name in names {
            if name != branch && !chain.iter().any(|c| c == name) {
                chain.push(name.clone());
            }
        }
    }
    chain.push(default.to_string());
    chain.reverse();
    chain
}

/// True if the worktree at `path` has uncommitted changes (tracked edits or
/// untracked-but-not-ignored files; gitignored files don't count).
fn is_dirty(path: &Path) -> bool {
    // Like is_ignored, this bypasses git_run/git_capture on purpose: we only
    // want the porcelain output and must tolerate a nonzero exit, which
    // git_capture would turn into an error.
    Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// cd / shell integration
// ---------------------------------------------------------------------------

/// Print the path of the worktree checked out for `branch` to stdout, for use
/// as `cd "$(tonic cd <branch>)"` (see `shell_init`).
pub fn cd(branch: &str) -> Result<()> {
    let repo = Repo::discover()?;
    let cfg = Config::load(&repo)?;
    let path = resolve_worktree(&repo, &cfg, branch)?;
    println!("{}", path.display());
    Ok(())
}

/// Print a shell function that wraps `tonic` so that `add`/`cd` change the
/// shell's directory. tonic can't cd its parent shell itself (it's a child
/// process); the sourced function captures the path tonic prints and cd's.
pub fn shell_init(shell: &str) -> Result<()> {
    let script = shell_wrapper(shell)
        .ok_or_else(|| anyhow!("unsupported shell '{shell}' (expected bash, zsh, or fish)"))?;
    print!("{script}");
    Ok(())
}

fn shell_wrapper(shell: &str) -> Option<&'static str> {
    match shell {
        "bash" | "zsh" => Some(BASH_ZSH_WRAPPER),
        "fish" => Some(FISH_WRAPPER),
        _ => None,
    }
}

// add/cd (and rm when it removes the current worktree) print a path on stdout
// (status is on stderr), so the wrapper captures stdout and cd's when non-empty;
// everything else passes through. rm/remove usually print nothing → no cd.
// Uses `local`, so this is bash/zsh only — not portable to a pure POSIX sh.
const BASH_ZSH_WRAPPER: &str = r#"tonic() {
    case "$1" in
        add|cd|rm|remove)
            # --help prints to stdout at exit 0; don't capture it as a path.
            case " $* " in
                *" -h "*|*" --help "*) command tonic "$@"; return ;;
            esac
            local __tonic_dir
            __tonic_dir="$(command tonic "$@")" || return
            [ -n "$__tonic_dir" ] && cd "$__tonic_dir"
            ;;
        *)
            command tonic "$@"
            ;;
    esac
}
"#;

const FISH_WRAPPER: &str = r#"function tonic
    switch $argv[1]
        case add cd rm remove
            # --help prints to stdout at exit 0; don't capture it as a path.
            if contains -- -h $argv; or contains -- --help $argv
                command tonic $argv
                return
            end
            set -l __tonic_dir (command tonic $argv); or return
            test -n "$__tonic_dir"; and cd $__tonic_dir
        case '*'
            command tonic $argv
    end
end
"#;

struct Worktree {
    path: PathBuf,
    /// Branch name, or "detached". Display label; use `bare` to test the anchor.
    label: String,
    /// True only for the bare-repo anchor entry (the standalone `bare` line),
    /// not for a branch that happens to be named "bare".
    bare: bool,
}

/// Parse `git worktree list --porcelain`. Each block is a `worktree <path>`
/// line plus `branch refs/heads/<name>`, or a standalone `bare` / `detached`.
fn parse_worktrees(porcelain: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut label = String::from("detached");
    let mut bare = false;
    for line in porcelain.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            if let Some(prev) = path.take() {
                out.push(Worktree {
                    path: prev,
                    label: std::mem::replace(&mut label, "detached".into()),
                    bare: std::mem::take(&mut bare),
                });
            }
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            label = b.to_string();
        } else if line == "bare" {
            // Standalone `bare` line = the anchor entry; distinct from a branch
            // literally named "bare" (which arrives as `branch refs/heads/bare`).
            bare = true;
            label = "bare".to_string();
        } else if line == "detached" {
            label = "detached".to_string();
        }
    }
    if let Some(p) = path {
        out.push(Worktree { path: p, label, bare });
    }
    out
}

/// Path of the worktree checked out for `branch`, if any (never the bare anchor).
fn parse_worktree(porcelain: &str, branch: &str) -> Option<PathBuf> {
    parse_worktrees(porcelain)
        .into_iter()
        .find(|w| !w.bare && w.label == branch)
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
        eprintln!("[{event}] {cmd}");
        let mut command = Command::new("sh");
        command.arg("-c").arg(&cmd).current_dir(worktree);
        // Hook stdout → our stderr (streamed, so long hooks like npm install
        // stay live) so it can't pollute the worktree path on tonic's stdout.
        redirect_stdout_to_stderr(&mut command);
        let status = command
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
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(dir);
    // git worktree add prints "HEAD is now at ..." to stdout; keep it (and any
    // other git chatter) off tonic's stdout so the worktree path stays clean.
    redirect_stdout_to_stderr(&mut cmd);
    let status = cmd.status().context("running git")?;
    if !status.success() {
        bail!("git {} failed", args.join(" "));
    }
    Ok(())
}

/// Route a child's stdout to our stderr, so subprocess chatter never lands on
/// tonic's stdout (which carries the worktree path for shell `cd` integration).
#[cfg(unix)]
fn redirect_stdout_to_stderr(cmd: &mut Command) {
    if let Ok(fd) = std::io::stderr().as_fd().try_clone_to_owned() {
        cmd.stdout(std::process::Stdio::from(fd));
    }
}

#[cfg(not(unix))]
fn redirect_stdout_to_stderr(_cmd: &mut Command) {}

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
// Tests may unwrap and index freely — the deny-panic lints target production.
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn shell_wrapper_per_shell() {
        assert!(shell_wrapper("zsh").unwrap().contains("tonic()"));
        assert!(shell_wrapper("bash").unwrap().contains("tonic()"));
        assert!(shell_wrapper("fish").unwrap().contains("function tonic"));
        assert!(shell_wrapper("elvish").is_none());
    }

    #[test]
    fn render_substitutes() {
        let out = render("{repo}.git/{branch}", &[("repo", "tonic"), ("branch", "feat/x")]);
        assert_eq!(out, "tonic.git/feat/x");
    }

    #[test]
    fn merge_later_wins_per_key() {
        let mut base = Config {
            worktree_path: Some("a".into()),
            ..Config::default()
        };
        base.merge(Config {
            worktree_path: Some("b".into()),
            ..Config::default()
        });
        assert_eq!(base.worktree_path.as_deref(), Some("b"));
    }

    #[test]
    fn worktree_base_normal_vs_bare() {
        // normal: base is the parent of the main working tree
        assert_eq!(
            worktree_base(false, Path::new("/src/myrepo")),
            PathBuf::from("/src")
        );
        // bare: base is the bare dir itself (siblings land inside it)
        assert_eq!(
            worktree_base(true, Path::new("/src/myrepo.git")),
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
    fn worktree_path_default_flattens_slash() {
        let repo = Repo {
            root: None,
            git_dir: PathBuf::from("/x/r/.git"),
            main: PathBuf::from("/x/r"),
            name: "r".into(),
            bare: false,
        };
        // default template "{repo}-{branch}", resolved against the main worktree's
        // parent; "feat/x" flattens to one dir.
        assert_eq!(
            worktree_path_for(&repo, &Config::default(), "feat/x"),
            PathBuf::from("/x/r-feat-x")
        );
    }

    #[test]
    fn new_branch_args_from_head_and_base() {
        let mut a = vec!["worktree".to_string(), "add".to_string()];
        new_branch_args(&mut a, "foo", "/p", None);
        assert_eq!(a, ["worktree", "add", "-b", "foo", "/p"]);

        let mut b: Vec<String> = Vec::new();
        new_branch_args(&mut b, "foo", "/p", Some("main"));
        assert_eq!(b, ["-b", "foo", "/p", "main"]);
    }

    #[test]
    fn bare_anchor_distinct_from_branch_named_bare() {
        let out = "worktree /repo.git\nbare\n\n\
                   worktree /repo.git/bare\nHEAD abc\nbranch refs/heads/bare\n";
        let wts = parse_worktrees(out);
        // anchor: bare flag set, not a resolvable branch
        assert!(wts[0].bare);
        // the branch literally named "bare": not the anchor, and cd finds it
        assert!(!wts[1].bare);
        assert_eq!(parse_worktree(out, "bare"), Some(PathBuf::from("/repo.git/bare")));
    }

    #[test]
    fn mode_defaults_to_copy() {
        let cfg = Config {
            include: Some(vec![Include { pattern: "node_modules".into(), mode: Mode::Symlink }]),
            ..Config::default()
        };
        assert_eq!(cfg.mode_for("node_modules"), Mode::Symlink);
        assert_eq!(cfg.mode_for(".env"), Mode::Copy);
    }
}
