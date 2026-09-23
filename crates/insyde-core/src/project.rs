//! Projects (repositories) and their worktrees, as shown in the sidebar.

use crate::git::{self, DiffStat};
use crate::store::ProjectRow;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Project {
    pub root: PathBuf,
    pub name: String,
    pub base: String,
    /// "Expo", "Next.js", "Rust"… detected from manifest files.
    pub stack: String,
    pub worktrees: Vec<Worktree>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WtStatus {
    /// Nothing notable.
    Plain,
    /// Has an open pull request.
    Pr,
    /// Checks failing or merge conflicts.
    Warn,
}

#[derive(Clone, Debug)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
    pub head: String,
    /// The repository's original checkout (★ in the sidebar).
    pub primary: bool,
    pub stat: DiffStat,
    pub last_commit: Option<i64>,
    pub pr: Option<u32>,
    pub status: WtStatus,
}

impl Project {
    pub fn letter(&self) -> String {
        self.name
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_ascii_uppercase().to_string())
            .unwrap_or_else(|| "?".into())
    }

    pub fn from_row(row: &ProjectRow) -> Self {
        Self {
            root: row.path.clone(),
            name: row.name.clone(),
            base: row.base.clone(),
            // Remote stacks are detected during the (background) scan.
            stack: if crate::remote::is_remote(&row.path) {
                "Remote".into()
            } else {
                detect_stack(&row.path)
            },
            worktrees: vec![],
        }
    }

    /// Validate a folder and produce its store row.
    pub fn probe(path: &Path) -> anyhow::Result<ProjectRow> {
        let root = git::repo_root(path)?;
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repo".into());
        let base = git::default_branch(&root);
        Ok(ProjectRow {
            path: root,
            name,
            base,
        })
    }

    /// Re-scan worktrees and their stats. Blocking; call off the UI thread.
    pub fn scan(&mut self, prs: &std::collections::HashMap<String, (u32, bool)>) {
        let Ok(entries) = git::list_worktrees(&self.root) else {
            return;
        };
        if crate::remote::is_remote(&self.root) {
            self.stack = format!("{} · SSH", detect_stack(&self.root));
        }
        self.worktrees = entries
            .into_iter()
            .map(|e| {
                let branch = e
                    .branch
                    .clone()
                    .unwrap_or_else(|| format!("detached@{}", e.head));
                let files = git::changed_files(&e.path, &self.base);
                let stat = git::diff_stat(&files);
                let pr = prs.get(&branch).copied();
                let conflicted = git::has_conflicts(&e.path);
                let status = match (conflicted, pr) {
                    (true, _) | (_, Some((_, true))) => WtStatus::Warn,
                    (_, Some(_)) => WtStatus::Pr,
                    _ => WtStatus::Plain,
                };
                Worktree {
                    last_commit: git::last_commit_time(&e.path),
                    path: e.path,
                    branch,
                    head: e.head,
                    primary: e.is_main,
                    stat,
                    pr: pr.map(|p| p.0),
                    status,
                }
            })
            .collect();
    }
}

impl Worktree {
    /// "#212 · 3m ago" line under the branch name.
    pub fn meta(&self) -> String {
        let t = self.last_commit.map(git::ago).unwrap_or_default();
        match self.pr {
            Some(n) => format!("#{n} · {t}"),
            None => t,
        }
    }
}

pub fn detect_stack(root: &Path) -> String {
    let remote = crate::remote::is_remote(root);
    // One listing instead of a probe per manifest when the repo is remote.
    let names: Vec<String> = if remote {
        crate::remote::list_dir(root)
            .into_iter()
            .map(|(n, _)| n)
            .collect()
    } else {
        vec![]
    };
    let has = |f: &str| {
        if remote {
            names.iter().any(|n| n == f)
        } else {
            root.join(f).exists()
        }
    };
    let read = |f: &str| {
        if has(f) {
            crate::remote::read_to_string(&root.join(f)).ok()
        } else {
            None
        }
    };
    if let Some(pkg) = read("package.json") {
        for (dep, label) in [
            ("\"expo\"", "Expo"),
            ("\"next\"", "Next.js"),
            ("\"@remix-run", "Remix"),
            ("\"svelte", "Svelte"),
            ("\"vue\"", "Vue"),
            ("\"react-native\"", "React Native"),
            ("\"electron\"", "Electron"),
            ("\"vite\"", "Vite"),
            ("\"react\"", "React"),
        ] {
            if pkg.contains(dep) {
                return label.into();
            }
        }
        return "Node".into();
    }
    for (f, label) in [
        ("Cargo.toml", "Rust"),
        ("go.mod", "Go"),
        ("pyproject.toml", "Python"),
        ("requirements.txt", "Python"),
        ("Package.swift", "Swift"),
        ("build.gradle.kts", "Kotlin"),
        ("build.gradle", "Java"),
        ("pom.xml", "Java"),
        ("Gemfile", "Ruby"),
        ("mix.exs", "Elixir"),
    ] {
        if has(f) {
            return label.into();
        }
    }
    "Repository".into()
}

/// The project's "run" command and its button label (`pnpm run dev`, `cargo run`…).
/// Works for local and remote worktrees; blocking.
pub fn run_command(path: &Path) -> Option<(String, String)> {
    let names: Vec<String> = crate::remote::list_dir(path)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let has = |f: &str| names.iter().any(|n| n == f);
    let read = |f: &str| {
        if has(f) {
            crate::remote::read_to_string(&path.join(f)).ok()
        } else {
            None
        }
    };
    if let Some(pkg) = read("package.json") {
        let pm = if has("pnpm-lock.yaml") {
            "pnpm"
        } else if has("bun.lockb") || has("bun.lock") {
            "bun"
        } else if has("yarn.lock") {
            "yarn"
        } else {
            "npm"
        };
        for s in ["dev", "start", "serve"] {
            if pkg.contains(&format!("\"{s}\":")) {
                return Some((format!("{pm} run {s}"), format!("Run {s}")));
            }
        }
    }
    if has("Cargo.toml") {
        return Some(("cargo run".into(), "Run cargo".into()));
    }
    if has("go.mod") {
        return Some(("go run .".into(), "Run go".into()));
    }
    if read("Makefile").is_some_and(|m| m.contains("\nrun:") || m.starts_with("run:")) {
        return Some(("make run".into(), "Run make".into()));
    }
    None
}
