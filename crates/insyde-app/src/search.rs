//! Worktree content search (ripgrep-style walk that respects .gitignore).

use std::io::{BufRead, BufReader};
use std::path::Path;

/// Case-insensitive literal search. Returns (relative path, 1-based line, text).
pub fn grep(root: &Path, query: &str, limit: usize) -> Vec<(String, usize, String)> {
    let needle = query.to_lowercase();
    let mut out = Vec::new();
    let walker = ignore::WalkBuilder::new(root).hidden(true).git_ignore(true).build();
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
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned();
        if rel.to_lowercase().contains(&needle) {
            out.push((rel.clone(), 1, "(file name match)".into()));
        }
        let Ok(f) = std::fs::File::open(path) else { continue };
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
