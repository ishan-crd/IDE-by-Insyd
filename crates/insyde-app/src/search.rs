//! Worktree content search (ripgrep-style walk that respects .gitignore).

use std::io::{BufRead, BufReader};
use std::path::Path;

/// Case-insensitive literal search. Returns (relative path, 1-based line, text).
pub fn grep(root: &Path, query: &str, limit: usize) -> Vec<(String, usize, String)> {
    if insyde_core::remote::is_remote(root) {
        return remote_grep(root, query, limit);
    }
    let needle = query.to_lowercase();
    let mut out = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .build();
    for entry in walker.filter_map(|e| e.ok()) {
        if out.len() >= limit {
            break;
        }
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        if entry.metadata().map(|m| m.len() > 2 << 20).unwrap_or(true) {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        if rel.to_lowercase().contains(&needle) {
            out.push((rel.clone(), 1, "(file name match)".into()));
        }
        let Ok(f) = std::fs::File::open(path) else {
            continue;
        };
        for (i, line) in BufReader::new(f).lines().enumerate() {
            let Ok(line) = line else { break }; // binary / invalid UTF-8
            if line.to_lowercase().contains(&needle) {
                out.push((rel.clone(), i + 1, line.trim().chars().take(200).collect()));
                if out.len() >= limit {
                    break;
                }
            }
        }
    }
    out
}

/// Remote search: `git grep` on the host (tracked and untracked, ignoring .gitignore'd files).
fn remote_grep(root: &Path, query: &str, limit: usize) -> Vec<(String, usize, String)> {
    let max = limit.to_string();
    let out = insyde_core::git::run(
        root,
        &[
            "grep",
            "-n",
            "-I",
            "-i",
            "-F",
            "--untracked",
            "--max-count",
            "5",
            "-e",
            query,
        ],
    )
    .unwrap_or_default();
    let _ = max;
    out.lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, ':');
            let (p, n, t) = (it.next()?, it.next()?, it.next()?);
            Some((
                p.to_string(),
                n.parse().ok()?,
                t.trim().chars().take(200).collect(),
            ))
        })
        .take(limit)
        .collect()
}
