//! Project Brain: a persistent context graph of a repository's main branch.
//!
//! Modeled on SiYuan's storage: typed nodes (SiYuan's `blocks`), directed
//! links (`refs`), and an external-content FTS5 index over node names,
//! summaries and user notes (`blocks_fts`). Notes support `[[Node name]]`
//! references that become graph links, like SiYuan block refs.
//!
//! The index is built from `main` via `git ls-tree`/`git cat-file`, so it
//! never touches worktrees or needs a checkout. Every agent session can be
//! seeded with a compact digest ([`Brain::digest`]) sized to a token budget.

pub mod index;
pub mod layout;

use anyhow::Result;
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    Root,
    Module,
    File,
    Symbol,
    Decision,
    Convention,
    Api,
    Pr,
    Doc,
}

impl Kind {
    pub const ALL: [Kind; 8] = [
        Kind::Module,
        Kind::File,
        Kind::Symbol,
        Kind::Decision,
        Kind::Convention,
        Kind::Api,
        Kind::Pr,
        Kind::Doc,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Root => "Project",
            Kind::Module => "Module",
            Kind::File => "File",
            Kind::Symbol => "Symbol",
            Kind::Decision => "Decision",
            Kind::Convention => "Convention",
            Kind::Api => "API",
            Kind::Pr => "PR",
            Kind::Doc => "Doc",
        }
    }
    /// Short chip text, as in the design.
    pub fn chip(self) -> &'static str {
        match self {
            Kind::Root => "P",
            Kind::Module => "M",
            Kind::File => "F",
            Kind::Symbol => "ƒ",
            Kind::Decision => "D",
            Kind::Convention => "C",
            Kind::Api => "A",
            Kind::Pr => "PR",
            Kind::Doc => "Dc",
        }
    }
    fn code(self) -> i64 {
        self as i64
    }
    fn from_code(c: i64) -> Kind {
        [
            Kind::Root,
            Kind::Module,
            Kind::File,
            Kind::Symbol,
            Kind::Decision,
            Kind::Convention,
            Kind::Api,
            Kind::Pr,
            Kind::Doc,
        ]
        .get(c as usize)
        .copied()
        .unwrap_or(Kind::Doc)
    }
    /// Kinds pinned into every agent context by default.
    pub fn pinned_by_default(self) -> bool {
        matches!(self, Kind::Decision | Kind::Convention)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    Hub,
    Child,
    Sym,
    Cross,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: usize,
    /// Stable identity across rebuilds ("file:src/a.ts").
    pub key: String,
    pub kind: Kind,
    pub name: String,
    /// Index into `Graph::groups`, `None` for the root.
    pub group: Option<usize>,
    pub summary: String,
    pub path: Option<String>,
    pub tokens: u32,
    pub changed: Option<i64>,
    pub pinned: bool,
    pub uses: u32,
    pub note: String,
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<(usize, usize, EdgeKind)>,
    pub groups: Vec<String>,
    pub sha: String,
    pub built_at: i64,
}

impl Graph {
    pub fn neighbors(&self) -> Vec<Vec<usize>> {
        let mut nb = vec![Vec::new(); self.nodes.len()];
        for &(a, b, _) in &self.edges {
            if a < nb.len() && b < nb.len() && a != b {
                if !nb[a].contains(&b) {
                    nb[a].push(b);
                }
                if !nb[b].contains(&a) {
                    nb[b].push(a);
                }
            }
        }
        nb
    }

    pub fn total_tokens(&self) -> u64 {
        self.nodes.iter().map(|n| n.tokens as u64).sum()
    }
}

/// Progress of a build: (percent, label).
pub type Progress = Arc<dyn Fn(u8, String) + Send + Sync>;

pub struct Brain {
    conn: Mutex<Connection>,
    pub repo: PathBuf,
    pub base: String,
}

const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS nodes(
  id INTEGER PRIMARY KEY, key TEXT UNIQUE NOT NULL, kind INTEGER NOT NULL, name TEXT NOT NULL,
  grp INTEGER, summary TEXT NOT NULL DEFAULT '', path TEXT, tokens INTEGER NOT NULL DEFAULT 0,
  changed INTEGER, pinned INTEGER, uses INTEGER NOT NULL DEFAULT 0, note TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS refs(a INTEGER NOT NULL, b INTEGER NOT NULL, kind INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS groups(id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE VIRTUAL TABLE IF NOT EXISTS nodes_fts USING fts5(name, summary, note, content='nodes', content_rowid='id', tokenize='unicode61 remove_diacritics 2');
"#;

fn db_path(repo: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    repo.hash(&mut h);
    let dir = crate::store::data_dir().join("brains");
    let _ = std::fs::create_dir_all(&dir);
    let name = repo
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir.join(format!("{name}-{:016x}.db", h.finish()))
}

impl Brain {
    /// Open the brain database for a repository (does not build it).
    pub fn open(repo: &Path, base: &str) -> Result<Self> {
        let conn = Connection::open(db_path(repo))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
            repo: repo.to_path_buf(),
            base: base.to_string(),
        })
    }

    pub fn exists(repo: &Path) -> bool {
        let p = db_path(repo);
        p.exists()
            && Connection::open(&p)
                .and_then(|c| c.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get::<_, i64>(0)))
                .map(|n| n > 0)
                .unwrap_or(false)
    }

    /// Build (or update) the index from the base branch, keeping pins,
    /// notes and usage counts of nodes that still exist.
    pub fn build(&self, progress: Progress) -> Result<Graph> {
        let old = self.load().unwrap_or_default();
        let mut g = index::build(&self.repo, &self.base, progress.clone())?;
        // Merge user state by stable key.
        let by_key: std::collections::HashMap<&str, &Node> =
            old.nodes.iter().map(|n| (n.key.as_str(), n)).collect();
        for n in &mut g.nodes {
            if let Some(o) = by_key.get(n.key.as_str()) {
                n.pinned = o.pinned;
                n.uses = o.uses;
                n.note = o.note.clone();
            }
        }
        // Re-link note references ([[Name]]) after the merge.
        let notes: Vec<(usize, String)> = g
            .nodes
            .iter()
            .filter(|n| !n.note.is_empty())
            .map(|n| (n.id, n.note.clone()))
            .collect();
        for (id, note) in notes {
            for target in note_refs(&note) {
                if let Some(t) = g
                    .nodes
                    .iter()
                    .find(|n| n.name.eq_ignore_ascii_case(&target))
                {
                    g.edges.push((id, t.id, EdgeKind::Cross));
                }
            }
        }
        progress(96, "Writing index…".into());
        self.save(&g)?;
        progress(100, "Done".into());
        Ok(g)
    }

    fn save(&self, g: &Graph) -> Result<()> {
        let mut c = self.conn.lock();
        let tx = c.transaction()?;
        tx.execute_batch("DELETE FROM refs; DELETE FROM groups; DELETE FROM nodes; INSERT INTO nodes_fts(nodes_fts) VALUES('delete-all');")?;
        {
            let mut ins = tx.prepare(
                "INSERT INTO nodes(id,key,kind,name,grp,summary,path,tokens,changed,pinned,uses,note) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            )?;
            let mut fts =
                tx.prepare("INSERT INTO nodes_fts(rowid,name,summary,note) VALUES(?1,?2,?3,?4)")?;
            for n in &g.nodes {
                ins.execute(params![
                    n.id as i64,
                    n.key,
                    n.kind.code(),
                    n.name,
                    n.group.map(|x| x as i64),
                    n.summary,
                    n.path,
                    n.tokens,
                    n.changed,
                    n.pinned as i64,
                    n.uses,
                    n.note
                ])?;
                fts.execute(params![n.id as i64, n.name, n.summary, n.note])?;
            }
            let mut e = tx.prepare("INSERT INTO refs(a,b,kind) VALUES(?1,?2,?3)")?;
            for &(a, b, k) in &g.edges {
                e.execute(params![a as i64, b as i64, k as i64])?;
            }
            let mut gr = tx.prepare("INSERT INTO groups(id,name) VALUES(?1,?2)")?;
            for (i, name) in g.groups.iter().enumerate() {
                gr.execute(params![i as i64, name])?;
            }
            tx.execute(
                "INSERT OR REPLACE INTO meta(key,value) VALUES('sha',?1)",
                params![g.sha],
            )?;
            tx.execute(
                "INSERT OR REPLACE INTO meta(key,value) VALUES('built_at',?1)",
                params![g.built_at.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load(&self) -> Result<Graph> {
        let c = self.conn.lock();
        let meta = |k: &str| -> String {
            c.query_row("SELECT value FROM meta WHERE key=?1", params![k], |r| {
                r.get(0)
            })
            .unwrap_or_default()
        };
        let sha = meta("sha");
        let built_at = meta("built_at").parse().unwrap_or(0);
        let mut st = c.prepare("SELECT id,key,kind,name,grp,summary,path,tokens,changed,pinned,uses,note FROM nodes ORDER BY id")?;
        let nodes = st
            .query_map([], |r| {
                let kind = Kind::from_code(r.get(2)?);
                let pinned: Option<i64> = r.get(9)?;
                Ok(Node {
                    id: r.get::<_, i64>(0)? as usize,
                    key: r.get(1)?,
                    kind,
                    name: r.get(3)?,
                    group: r.get::<_, Option<i64>>(4)?.map(|g| g as usize),
                    summary: r.get(5)?,
                    path: r.get(6)?,
                    tokens: r.get(7)?,
                    changed: r.get(8)?,
                    pinned: pinned.map(|p| p != 0).unwrap_or(kind.pinned_by_default()),
                    uses: r.get(10)?,
                    note: r.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut st = c.prepare("SELECT a,b,kind FROM refs")?;
        let edges = st
            .query_map([], |r| {
                let k = match r.get::<_, i64>(2)? {
                    0 => EdgeKind::Hub,
                    1 => EdgeKind::Child,
                    2 => EdgeKind::Sym,
                    _ => EdgeKind::Cross,
                };
                Ok((
                    r.get::<_, i64>(0)? as usize,
                    r.get::<_, i64>(1)? as usize,
                    k,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut st = c.prepare("SELECT name FROM groups ORDER BY id")?;
        let groups = st
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(Graph {
            nodes,
            edges,
            groups,
            sha,
            built_at,
        })
    }

    pub fn set_pinned(&self, id: usize, pinned: bool) {
        let _ = self.conn.lock().execute(
            "UPDATE nodes SET pinned=?2 WHERE id=?1",
            params![id as i64, pinned as i64],
        );
    }

    /// Save a user note and index it; returns the ids it references.
    pub fn set_note(&self, id: usize, note: &str, graph: &Graph) -> Vec<usize> {
        let c = self.conn.lock();
        let old: (String, String, String) = c
            .query_row(
                "SELECT name,summary,note FROM nodes WHERE id=?1",
                params![id as i64],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or_default();
        let _ = c.execute(
            "INSERT INTO nodes_fts(nodes_fts,rowid,name,summary,note) VALUES('delete',?1,?2,?3,?4)",
            params![id as i64, old.0, old.1, old.2],
        );
        let _ = c.execute(
            "UPDATE nodes SET note=?2 WHERE id=?1",
            params![id as i64, note],
        );
        let _ = c.execute(
            "INSERT INTO nodes_fts(rowid,name,summary,note) VALUES(?1,?2,?3,?4)",
            params![id as i64, old.0, old.1, note],
        );
        let targets: Vec<usize> = note_refs(note)
            .iter()
            .filter_map(|t| {
                graph
                    .nodes
                    .iter()
                    .find(|n| n.name.eq_ignore_ascii_case(t))
                    .map(|n| n.id)
            })
            .collect();
        for &t in &targets {
            let _ = c.execute(
                "INSERT INTO refs(a,b,kind) VALUES(?1,?2,3)",
                params![id as i64, t as i64],
            );
        }
        targets
    }

    /// Full-text search (SiYuan-style FTS over name, summary, note).
    pub fn search(&self, query: &str, limit: usize) -> Vec<usize> {
        let q = fts_query(query);
        if q.is_empty() {
            return vec![];
        }
        let c = self.conn.lock();
        let Ok(mut st) = c.prepare("SELECT rowid FROM nodes_fts WHERE nodes_fts MATCH ?1 ORDER BY bm25(nodes_fts, 5.0, 1.0, 2.0) LIMIT ?2") else {
            return vec![];
        };
        st.query_map(params![q, limit as i64], |r| r.get::<_, i64>(0))
            .map(|it| it.filter_map(|x| x.ok()).map(|x| x as usize).collect())
            .unwrap_or_default()
    }

    fn bump_uses(&self, ids: &[usize]) {
        let c = self.conn.lock();
        for id in ids {
            let _ = c.execute(
                "UPDATE nodes SET uses=uses+1 WHERE id=?1",
                params![*id as i64],
            );
        }
    }

    /// Compact Markdown context for an agent, within `budget_tokens`.
    /// Pinned nodes always come first; then the areas most relevant to `task`.
    pub fn digest(&self, g: &Graph, project: &str, task: &str, budget_tokens: usize) -> String {
        let budget = budget_tokens * 4;
        let mut out = String::with_capacity(budget.min(1 << 20));
        out.push_str(&format!(
            "# Project Brain — {project}\nBuilt from `{}` @ {}. Treat decisions and conventions as constraints unless the user overrides them.\n\n",
            self.base, g.sha
        ));
        // Areas overview.
        out.push_str("## Areas\n");
        for (gi, name) in g.groups.iter().enumerate() {
            let files: Vec<&str> = g
                .nodes
                .iter()
                .filter(|n| n.group == Some(gi) && n.kind == Kind::File)
                .take(6)
                .map(|n| n.path.as_deref().unwrap_or(&n.name))
                .collect();
            if !files.is_empty() {
                out.push_str(&format!("- **{name}**: {}\n", files.join(", ")));
            }
        }
        let section = |out: &mut String, title: &str, kinds: &[Kind], only_pinned: bool| {
            let items: Vec<&Node> = g
                .nodes
                .iter()
                .filter(|n| kinds.contains(&n.kind) && (!only_pinned || n.pinned))
                .collect();
            if items.is_empty() {
                return;
            }
            out.push_str(&format!("\n## {title}\n"));
            for n in items {
                if out.len() > budget {
                    break;
                }
                out.push_str(&format!("- {}: {}\n", n.name, first_line(&n.summary)));
                if !n.note.is_empty() {
                    out.push_str(&format!("  Note: {}\n", n.note.replace('\n', " ")));
                }
            }
        };
        section(&mut out, "Decisions", &[Kind::Decision], true);
        section(&mut out, "Conventions", &[Kind::Convention], true);
        // Other pinned nodes.
        let pinned_other: Vec<&Node> = g
            .nodes
            .iter()
            .filter(|n| n.pinned && !matches!(n.kind, Kind::Decision | Kind::Convention))
            .collect();
        if !pinned_other.is_empty() {
            out.push_str("\n## Pinned\n");
            for n in pinned_other {
                out.push_str(&format!(
                    "- {} ({}): {}\n",
                    n.name,
                    n.kind.label(),
                    n.summary.replace('\n', " ")
                ));
            }
        }
        section(&mut out, "API", &[Kind::Api], false);
        // Task-relevant files and notes.
        let hits = self.search(task, 40);
        if !hits.is_empty() {
            out.push_str("\n## Relevant to this task\n");
            let mut used = Vec::new();
            for id in hits {
                let Some(n) = g.nodes.get(id) else { continue };
                if out.len() > budget {
                    break;
                }
                used.push(id);
                out.push_str(&format!(
                    "### {} ({})\n{}\n",
                    n.path.as_deref().unwrap_or(&n.name),
                    n.kind.label(),
                    n.summary
                ));
                if !n.note.is_empty() {
                    out.push_str(&format!("Note: {}\n", n.note));
                }
            }
            self.bump_uses(&used);
        }
        let prs: Vec<&Node> = g
            .nodes
            .iter()
            .filter(|n| n.kind == Kind::Pr)
            .take(6)
            .collect();
        if !prs.is_empty() && out.len() < budget {
            out.push_str("\n## Recent merged work\n");
            for n in prs {
                out.push_str(&format!("- {}: {}\n", n.name, first_line(&n.summary)));
            }
        }
        if out.len() > budget {
            let mut cut = budget;
            while !out.is_char_boundary(cut) {
                cut -= 1;
            }
            out.truncate(cut);
        }
        out
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

/// `[[Target]]` references inside a note.
pub fn note_refs(note: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut rest = note;
    while let Some(i) = rest.find("[[") {
        let after = &rest[i + 2..];
        let Some(j) = after.find("]]") else { break };
        let t = after[..j].trim();
        if !t.is_empty() && t.len() < 200 {
            v.push(t.to_string());
        }
        rest = &after[j + 2..];
    }
    v
}

/// Turn free text into a safe FTS5 OR-query of prefix terms.
fn fts_query(text: &str) -> String {
    const STOP: &[&str] = &[
        "the", "and", "for", "with", "this", "that", "then", "fix", "make", "add", "run", "from",
        "into", "when", "what", "please",
    ];
    let mut terms: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() >= 3 && !STOP.contains(&w.to_lowercase().as_str()))
        .map(|w| format!("\"{}\"*", w.to_lowercase()))
        .collect();
    terms.dedup();
    terms.truncate(24);
    terms.join(" OR ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refs_and_query() {
        assert_eq!(
            note_refs("see [[Auth]] and [[ push.ts ]]"),
            vec!["Auth", "push.ts"]
        );
        assert_eq!(
            fts_query("Fix the Android permission layout"),
            "\"android\"* OR \"permission\"* OR \"layout\"*"
        );
    }
}
