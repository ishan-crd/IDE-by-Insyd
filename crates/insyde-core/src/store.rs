//! SQLite persistence: projects, UI settings, agent sessions and their
//! transcripts. One connection behind a mutex; every write is small, so
//! contention is negligible. WAL mode keeps UI-thread reads cheap.

use anyhow::Result;
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

pub fn data_dir() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| std::env::temp_dir());
    let d = base.join("InsyDE");
    let _ = std::fs::create_dir_all(&d);
    d
}

const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
CREATE TABLE IF NOT EXISTS projects(
  path TEXT PRIMARY KEY, name TEXT NOT NULL, base TEXT NOT NULL,
  sort INTEGER NOT NULL DEFAULT 0, added INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS kv(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS sessions(
  id INTEGER PRIMARY KEY AUTOINCREMENT, worktree TEXT NOT NULL, agent TEXT NOT NULL,
  title TEXT NOT NULL, acp_id TEXT, created INTEGER NOT NULL, updated INTEGER NOT NULL,
  closed INTEGER NOT NULL DEFAULT 0, tokens INTEGER NOT NULL DEFAULT 0, cost REAL NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS sessions_wt ON sessions(worktree, closed);
CREATE TABLE IF NOT EXISTS events(
  session INTEGER NOT NULL, seq INTEGER NOT NULL, body TEXT NOT NULL, PRIMARY KEY(session, seq));
"#;

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Clone, Debug)]
pub struct ProjectRow {
    pub path: PathBuf,
    pub name: String,
    pub base: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionRow {
    pub id: i64,
    pub worktree: PathBuf,
    pub agent: String,
    pub title: String,
    pub acp_id: Option<String>,
    pub tokens: i64,
    pub cost: f64,
}

impl Store {
    pub fn open_default() -> Result<Self> {
        Self::open(&data_dir().join("insyde.db"))
    }

    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    // ---- projects ----
    pub fn projects(&self) -> Result<Vec<ProjectRow>> {
        let c = self.conn.lock();
        let mut st = c.prepare("SELECT path,name,base FROM projects ORDER BY sort, added")?;
        let rows = st
            .query_map([], |r| Ok(ProjectRow { path: PathBuf::from(r.get::<_, String>(0)?), name: r.get(1)?, base: r.get(2)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn add_project(&self, p: &ProjectRow) -> Result<()> {
        let c = self.conn.lock();
        let sort: i64 = c.query_row("SELECT COALESCE(MAX(sort),0)+1 FROM projects", [], |r| r.get(0))?;
        c.execute(
            "INSERT OR IGNORE INTO projects(path,name,base,sort,added) VALUES(?1,?2,?3,?4,?5)",
            params![p.path.to_string_lossy(), p.name, p.base, sort, now()],
        )?;
        Ok(())
    }

    pub fn remove_project(&self, path: &Path) -> Result<()> {
        self.conn.lock().execute("DELETE FROM projects WHERE path=?1", params![path.to_string_lossy()])?;
        Ok(())
    }

    // ---- settings ----
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let c = self.conn.lock();
        let s: Option<String> = c.query_row("SELECT value FROM kv WHERE key=?1", params![key], |r| r.get(0)).optional().ok()??;
        serde_json::from_str(&s?).ok()
    }

    pub fn set<T: Serialize>(&self, key: &str, v: &T) {
        if let Ok(s) = serde_json::to_string(v) {
            let _ = self.conn.lock().execute("INSERT OR REPLACE INTO kv(key,value) VALUES(?1,?2)", params![key, s]);
        }
    }

    // ---- sessions ----
    pub fn create_session(&self, worktree: &Path, agent: &str, title: &str) -> Result<i64> {
        let c = self.conn.lock();
        let t = now();
        c.execute(
            "INSERT INTO sessions(worktree,agent,title,created,updated) VALUES(?1,?2,?3,?4,?4)",
            params![worktree.to_string_lossy(), agent, title, t],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn open_sessions(&self, worktree: &Path) -> Result<Vec<SessionRow>> {
        let c = self.conn.lock();
        let mut st = c.prepare(
            "SELECT id,worktree,agent,title,acp_id,tokens,cost FROM sessions WHERE worktree=?1 AND closed=0 ORDER BY id",
        )?;
        let rows = st
            .query_map(params![worktree.to_string_lossy()], |r| {
                Ok(SessionRow {
                    id: r.get(0)?,
                    worktree: PathBuf::from(r.get::<_, String>(1)?),
                    agent: r.get(2)?,
                    title: r.get(3)?,
                    acp_id: r.get(4)?,
                    tokens: r.get(5)?,
                    cost: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn update_session(&self, id: i64, title: Option<&str>, acp_id: Option<&str>, tokens: Option<i64>, cost: Option<f64>) {
        let c = self.conn.lock();
        let _ = c.execute(
            "UPDATE sessions SET title=COALESCE(?2,title), acp_id=COALESCE(?3,acp_id), tokens=COALESCE(?4,tokens),
             cost=COALESCE(?5,cost), updated=?6 WHERE id=?1",
            params![id, title, acp_id, tokens, cost, now()],
        );
    }

    /// Most recent sessions of a worktree, open or closed (session history).
    pub fn recent_sessions(&self, worktree: &Path, limit: usize) -> Vec<SessionRow> {
        let c = self.conn.lock();
        let Ok(mut st) = c.prepare(
            "SELECT id,worktree,agent,title,acp_id,tokens,cost FROM sessions WHERE worktree=?1 ORDER BY updated DESC LIMIT ?2",
        ) else {
            return vec![];
        };
        st.query_map(params![worktree.to_string_lossy(), limit as i64], |r| {
            Ok(SessionRow {
                id: r.get(0)?,
                worktree: PathBuf::from(r.get::<_, String>(1)?),
                agent: r.get(2)?,
                title: r.get(3)?,
                acp_id: r.get(4)?,
                tokens: r.get(5)?,
                cost: r.get(6)?,
            })
        })
        .map(|it| it.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
    }

    pub fn reopen_session(&self, id: i64) {
        let _ = self.conn.lock().execute("UPDATE sessions SET closed=0 WHERE id=?1", params![id]);
    }

    pub fn close_session(&self, id: i64) {
        let _ = self.conn.lock().execute("UPDATE sessions SET closed=1 WHERE id=?1", params![id]);
    }

    pub fn total_cost(&self) -> f64 {
        self.conn.lock().query_row("SELECT COALESCE(SUM(cost),0) FROM sessions", [], |r| r.get(0)).unwrap_or(0.0)
    }

    /// Append one transcript event. Called before the UI shows it, so a crash
    /// never loses what the user saw.
    pub fn append_event<T: Serialize>(&self, session: i64, seq: i64, ev: &T) {
        if let Ok(body) = serde_json::to_string(ev) {
            let _ = self.conn.lock().execute(
                "INSERT OR REPLACE INTO events(session,seq,body) VALUES(?1,?2,?3)",
                params![session, seq, body],
            );
        }
    }

    pub fn events<T: DeserializeOwned>(&self, session: i64) -> Vec<T> {
        let c = self.conn.lock();
        let Ok(mut st) = c.prepare("SELECT body FROM events WHERE session=?1 ORDER BY seq") else { return vec![] };
        st.query_map(params![session], |r| r.get::<_, String>(0))
            .map(|it| it.filter_map(|s| s.ok().and_then(|s| serde_json::from_str(&s).ok())).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let s = Store::in_memory().unwrap();
        s.add_project(&ProjectRow { path: "/tmp/x".into(), name: "x".into(), base: "main".into() }).unwrap();
        assert_eq!(s.projects().unwrap().len(), 1);
        s.set("theme", &"dark");
        assert_eq!(s.get::<String>("theme").as_deref(), Some("dark"));
        let id = s.create_session(Path::new("/tmp/x"), "claude", "t").unwrap();
        s.append_event(id, 0, &serde_json::json!({"k":1}));
        assert_eq!(s.events::<serde_json::Value>(id).len(), 1);
    }
}
