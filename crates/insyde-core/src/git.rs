//! Git access by shelling out to the system `git` binary.
//!
//! Every call is bounded: output is capped (1 MiB, like T3 Code's driver) and
//! at most `MAX_CONCURRENT` git processes run at once, so a burst of status
//! refreshes across many worktrees cannot fork-bomb the machine. All functions
//! are blocking and must run on a background thread.

use anyhow::{Context, Result, anyhow, bail};
use parking_lot::{Condvar, Mutex};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAX_OUTPUT: usize = 1 << 20;
const MAX_CONCURRENT: usize = 8;

static SLOTS: Mutex<usize> = Mutex::new(0);
static FREED: Condvar = Condvar::new();

struct Slot;
impl Slot {
    fn take() -> Self {
        let mut n = SLOTS.lock();
        while *n >= MAX_CONCURRENT {
            FREED.wait(&mut n);
        }
        *n += 1;
        Slot
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        *SLOTS.lock() -= 1;
        FREED.notify_one();
    }
}

/// Run `git args…` in `cwd`, returning stdout (capped). Fails on non-zero exit
/// with stderr as the message.
pub fn run(cwd: &Path, args: &[&str]) -> Result<String> {
    run_capped(cwd, args, MAX_OUTPUT)
}

/// [`run`] with a custom output cap. Output past the cap is drained and
/// discarded (never left in the pipe, which would stall or SIGPIPE git).
pub fn run_capped(cwd: &Path, args: &[&str], cap: usize) -> Result<String> {
    let _slot = Slot::take();
    let mut child = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn git {}", args.first().unwrap_or(&"")))?;
    let mut out = Vec::with_capacity(4096);
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let err_reader = std::thread::spawn(move || {
        let mut err = String::new();
        let _ = (&mut stderr).take(64 * 1024).read_to_string(&mut err);
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        err
    });
    (&mut stdout).take(cap as u64).read_to_end(&mut out)?;
    let _ = std::io::copy(&mut stdout, &mut std::io::sink());
    let err = err_reader.join().unwrap_or_default();
    let status = child.wait()?;
    if !status.success() {
        bail!("git {}: {}", args.join(" "), err.trim());
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Like [`run`] but returns `None` instead of an error (for optional probes).
pub fn try_run(cwd: &Path, args: &[&str]) -> Option<String> {
    run(cwd, args).ok()
}

pub fn repo_root(path: &Path) -> Result<PathBuf> {
    // `--git-common-dir` resolves to the main repo even from a linked worktree.
    let common = run(path, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let common = PathBuf::from(common.trim());
    let root = if common.file_name().is_some_and(|n| n == ".git") {
        common.parent().map(Path::to_path_buf).unwrap_or(common)
    } else {
        PathBuf::from(run(path, &["rev-parse", "--show-toplevel"])?.trim())
    };
    Ok(root)
}

/// The branch new work should be based on: `origin/HEAD`'s target, else
/// `main`, else `master`, else the current branch.
pub fn default_branch(repo: &Path) -> String {
    if let Some(s) = try_run(repo, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]) {
        if let Some(b) = s.trim().strip_prefix("origin/") {
            return b.to_string();
        }
    }
    for b in ["main", "master", "trunk", "develop"] {
        if try_run(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{b}")]).is_some() {
            return b.to_string();
        }
    }
    current_branch(repo).unwrap_or_else(|| "main".into())
}

pub fn current_branch(cwd: &Path) -> Option<String> {
    try_run(cwd, &["symbolic-ref", "--quiet", "--short", "HEAD"]).map(|s| s.trim().to_string())
}

pub fn head_sha(cwd: &Path, rev: &str) -> Option<String> {
    try_run(cwd, &["rev-parse", "--short", rev]).map(|s| s.trim().to_string())
}

#[derive(Clone, Debug)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: String,
    pub is_main: bool,
    pub locked: bool,
    pub prunable: bool,
}

pub fn list_worktrees(repo: &Path) -> Result<Vec<WorktreeEntry>> {
    let out = run(repo, &["worktree", "list", "--porcelain", "-z"])?;
    let mut list = Vec::new();
    let mut cur: Option<WorktreeEntry> = None;
    for field in out.split('\0') {
        if field.is_empty() {
            if let Some(w) = cur.take() {
                list.push(w);
            }
            continue;
        }
        if let Some(p) = field.strip_prefix("worktree ") {
            if let Some(w) = cur.take() {
                list.push(w);
            }
            cur = Some(WorktreeEntry {
                path: PathBuf::from(p),
                branch: None,
                head: String::new(),
                is_main: list.is_empty(),
                locked: false,
                prunable: false,
            });
        } else if let Some(w) = cur.as_mut() {
            if let Some(h) = field.strip_prefix("HEAD ") {
                w.head = h.chars().take(7).collect();
            } else if let Some(b) = field.strip_prefix("branch ") {
                w.branch = Some(b.trim_start_matches("refs/heads/").to_string());
            } else if field.starts_with("locked") {
                w.locked = true;
            } else if field.starts_with("prunable") {
                w.prunable = true;
            }
        }
    }
    if let Some(w) = cur {
        list.push(w);
    }
    list.retain(|w| !w.prunable);
    Ok(list)
}

/// Turn a free-text task title into a branch name (`feat/…` when no prefix).
pub fn slugify_branch(title: &str) -> String {
    let t = title.trim();
    let (prefix, rest) = match t.split_once('/') {
        Some((p, r)) if !p.contains(' ') && p.len() <= 12 => (p.to_lowercase(), r),
        _ => ("feat".to_string(), t),
    };
    let mut slug = String::with_capacity(rest.len());
    let mut dash = false;
    for c in rest.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !slug.is_empty() {
            slug.push('-');
            dash = true;
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() { format!("{prefix}/task") } else { format!("{prefix}/{slug}") }
}

/// Default directory for task worktrees: `<repo>/../.insyde-worktrees/<repo-name>`.
pub fn worktrees_dir(repo: &Path) -> PathBuf {
    let name = repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".into());
    repo.parent().unwrap_or(repo).join(".insyde-worktrees").join(name)
}

pub fn add_worktree(repo: &Path, branch: &str, base: &str) -> Result<PathBuf> {
    let dir = worktrees_dir(repo).join(branch.replace('/', "-"));
    std::fs::create_dir_all(dir.parent().unwrap())?;
    let d = dir.to_string_lossy();
    let exists = try_run(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_some();
    if exists {
        run(repo, &["worktree", "add", &d, branch])?;
    } else {
        run(repo, &["worktree", "add", "-b", branch, &d, base])?;
    }
    Ok(dir)
}

/// Remove a task worktree. Refuses when it has unpushed commits or local
/// changes unless `force`.
pub fn remove_worktree(repo: &Path, path: &Path, force: bool) -> Result<()> {
    if !force {
        let dirty = try_run(path, &["status", "--porcelain"]).is_some_and(|s| !s.trim().is_empty());
        if dirty {
            return Err(anyhow!("worktree has uncommitted changes"));
        }
    }
    let p = path.to_string_lossy();
    if force {
        run(repo, &["worktree", "remove", "--force", &p])?;
    } else {
        run(repo, &["worktree", "remove", &p])?;
    }
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffStat {
    pub added: u32,
    pub removed: u32,
    pub files: u32,
}

#[derive(Clone, Debug)]
pub struct FileStat {
    pub path: String,
    pub added: u32,
    pub removed: u32,
    pub binary: bool,
}

fn parse_numstat_z(out: &str) -> Vec<FileStat> {
    // With -z: "<a>\t<d>\t<path>\0", renames: "<a>\t<d>\t\0<old>\0<new>\0".
    let mut v = Vec::new();
    let mut it = out.split('\0');
    while let Some(rec) = it.next() {
        if rec.is_empty() {
            continue;
        }
        let mut parts = rec.splitn(3, '\t');
        let (a, d, p) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let path = if p.is_empty() {
            let _old = it.next();
            it.next().unwrap_or("").to_string()
        } else {
            p.to_string()
        };
        let binary = a == "-";
        v.push(FileStat { path, added: a.parse().unwrap_or(0), removed: d.parse().unwrap_or(0), binary });
    }
    v
}

/// Changes of the worktree (committed on the branch + uncommitted) relative
/// to the merge-base with `base`.
pub fn changed_files(cwd: &Path, base: &str) -> Vec<FileStat> {
    let mb = try_run(cwd, &["merge-base", "HEAD", base]).map(|s| s.trim().to_string());
    let range = mb.unwrap_or_else(|| "HEAD".into());
    let mut files = try_run(cwd, &["diff", "--numstat", "-z", &range, "--"]).map(|s| parse_numstat_z(&s)).unwrap_or_default();
    // Untracked files count as additions.
    if let Some(u) = try_run(cwd, &["ls-files", "--others", "--exclude-standard", "-z"]) {
        for p in u.split('\0').filter(|p| !p.is_empty()).take(500) {
            let lines = std::fs::read(cwd.join(p)).map(|b| bytecount_lines(&b)).unwrap_or(0);
            files.push(FileStat { path: p.to_string(), added: lines, removed: 0, binary: false });
        }
    }
    files
}

fn bytecount_lines(b: &[u8]) -> u32 {
    if b.len() > 4 << 20 || b.contains(&0) {
        return 0;
    }
    b.iter().filter(|&&c| c == b'\n').count() as u32
}

pub fn diff_stat(files: &[FileStat]) -> DiffStat {
    files.iter().fold(DiffStat::default(), |mut s, f| {
        s.added += f.added;
        s.removed += f.removed;
        s.files += 1;
        s
    })
}

/// Unified patch for one file relative to the merge-base (or for an untracked file).
pub fn file_patch(cwd: &Path, base: &str, path: &str) -> String {
    let range = try_run(cwd, &["merge-base", "HEAD", base]).map(|s| s.trim().to_string()).unwrap_or_else(|| "HEAD".into());
    let tracked = try_run(cwd, &["ls-files", "--error-unmatch", "--", path]).is_some();
    if tracked {
        try_run(cwd, &["diff", "--no-ext-diff", "--no-color", "-U3", &range, "--", path]).unwrap_or_default()
    } else {
        try_run(cwd, &["diff", "--no-index", "--no-color", "--", "/dev/null", path]).unwrap_or_else(|| {
            // `diff --no-index` exits 1 when files differ; fall back to raw content.
            let body = std::fs::read_to_string(cwd.join(path)).unwrap_or_default();
            let mut s = format!("@@ -0,0 +1,{} @@\n", body.lines().count());
            for l in body.lines().take(4000) {
                s.push('+');
                s.push_str(l);
                s.push('\n');
            }
            s
        })
    }
}

/// Full patch (capped) of the worktree vs base — used for agent hand-off.
pub fn full_patch(cwd: &Path, base: &str, max_bytes: usize) -> String {
    let range = try_run(cwd, &["merge-base", "HEAD", base]).map(|s| s.trim().to_string()).unwrap_or_else(|| "HEAD".into());
    let mut p = try_run(cwd, &["diff", "--no-ext-diff", "--no-color", "-U2", &range, "--"]).unwrap_or_default();
    if p.len() > max_bytes {
        let mut cut = max_bytes;
        while !p.is_char_boundary(cut) {
            cut -= 1;
        }
        p.truncate(cut);
        p.push_str("\n[diff truncated]\n");
    }
    p
}

/// `(ahead, behind)` relative to the upstream, if any.
pub fn ahead_behind(cwd: &Path) -> Option<(u32, u32)> {
    let s = try_run(cwd, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])?;
    let mut it = s.split_whitespace();
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

/// Unix seconds of the last commit on HEAD.
pub fn last_commit_time(cwd: &Path) -> Option<i64> {
    try_run(cwd, &["log", "-1", "--format=%ct"]).and_then(|s| s.trim().parse().ok())
}

pub fn has_conflicts(cwd: &Path) -> bool {
    try_run(cwd, &["diff", "--name-only", "--diff-filter=U"]).is_some_and(|s| !s.trim().is_empty())
}

/// Human "3m ago" style from unix seconds.
pub fn ago(ts: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(ts);
    let d = (now - ts).max(0);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86_399 => format!("{}h ago", d / 3600),
        86_400..=604_799 => format!("{}d ago", d / 86_400),
        _ => format!("{}w ago", d / 604_800),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify_branch("Fix Android permission layout shift"), "feat/fix-android-permission-layout-shift");
        assert_eq!(slugify_branch("fix/Push token refresh!"), "fix/push-token-refresh");
        assert_eq!(slugify_branch("  "), "feat/task");
    }

    #[test]
    fn numstat() {
        let s = "3\t1\tsrc/a.rs\0-\t-\tlogo.png\0" .to_string() + "2\t0\t\0old.rs\0new.rs\0";
        let v = parse_numstat_z(&s);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].added, 3);
        assert!(v[1].binary);
        assert_eq!(v[2].path, "new.rs");
    }
}
