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
        self.name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_else(|| "?".into())
    }

    pub fn from_row(row: &ProjectRow) -> Self {
        Self { root: row.path.clone(), name: row.name.clone(), base: row.base.clone(), stack: detect_stack(&row.path), worktrees: vec![] }
    }

    /// Validate a folder and produce its store row.
    pub fn probe(path: &Path) -> anyhow::Result<ProjectRow> {
        let root = git::repo_root(path)?;
        let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".into());
        let base = git::default_branch(&root);
        Ok(ProjectRow { path: root, name, base })
    }

    /// Re-scan worktrees and their stats. Blocking; call off the UI thread.
    pub fn scan(&mut self, prs: &std::collections::HashMap<String, (u32, bool)>) {
        let Ok(entries) = git::list_worktrees(&self.root) else { return };
        self.worktrees = entries
            .into_iter()
            .map(|e| {
                let branch = e.branch.clone().unwrap_or_else(|| format!("detached@{}", e.head));
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
    let read = |f: &str| std::fs::read_to_string(root.join(f)).ok();
    if let Some(pkg) = read("package.json") {
        for (dep, label) in [("\"expo\"", "Expo"), ("\"next\"", "Next.js"), ("\"@remix-run", "Remix"), ("\"svelte", "Svelte"),
            ("\"vue\"", "Vue"), ("\"react-native\"", "React Native"), ("\"electron\"", "Electron"), ("\"vite\"", "Vite"), ("\"react\"", "React")] {
            if pkg.contains(dep) {
                return label.into();
            }
        }
        return "Node".into();
    }
    for (f, label) in [("Cargo.toml", "Rust"), ("go.mod", "Go"), ("pyproject.toml", "Python"), ("requirements.txt", "Python"),
        ("Package.swift", "Swift"), ("build.gradle.kts", "Kotlin"), ("build.gradle", "Java"), ("pom.xml", "Java"), ("Gemfile", "Ruby"), ("mix.exs", "Elixir")] {
        if root.join(f).exists() {
            return label.into();
        }
    }
    "Repository".into()
}
