//! The web hub: everything a browser can see and do, shared by all connected
//! clients. It owns agent threads (ACP sessions) and browser terminals, so
//! work keeps running when a tab closes and any device can pick it up.
//!
//! Updates are pushed, not polled: an agent's notify marks its thread dirty,
//! and one flusher thread coalesces changes (~30 per second at most) into
//! small diffs ("item 12 changed, length is now 14") for subscribed clients.

use super::pty::{Program, WebTerm};
use crate::agents::acp::{AcpSession, Block, Launch, Policy};
use crate::agents::transcript::{Item, PermissionPrompt, Transcript};
use crate::agents::{AGENTS, AgentSpec};
use crate::brain::{Brain, Graph};
use crate::project::Project;
use crate::store::{Store, now};
use crate::{forge, git, remote, settings};
use anyhow::{Context as _, Result, anyhow, bail};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// A frame queued for one client's writer thread.
pub enum Out {
    Text(String),
    Binary(Vec<u8>),
    Pong(Vec<u8>),
    Close,
}

/// Binary frame tag: terminal output (`[1][u32 id][bytes]`).
pub const BIN_TERM: u8 = 1;

struct Client {
    tx: flume::Sender<Out>,
    /// The socket, kept to cut off a client that can't keep up.
    sock: std::net::TcpStream,
    threads: HashSet<i64>,
}

impl Client {
    fn kill(&self) {
        let _ = self.sock.shutdown(std::net::Shutdown::Both);
    }
}

/// Transcript tail re-checked on every flush. Older items are only
/// re-compared when a turn ends (tool results can land a little late).
const TAIL: usize = 24;

struct Live {
    id: i64,
    worktree: PathBuf,
    project: String,
    spec: &'static AgentSpec,
    transcript: Arc<Mutex<Transcript>>,
    session: Mutex<Option<AcpSession>>,
    acp_id: Mutex<Option<String>>,
    policy: Mutex<Policy>,
    sent_first: AtomicBool,
    /// Last state sent to clients: serialized items, meta, sidebar status.
    sent: Mutex<(Vec<String>, String, Status)>,
}

#[derive(Clone, Copy, Default, PartialEq, Serialize)]
struct Status {
    running: bool,
    attention: bool,
    error: bool,
}

#[derive(Serialize)]
struct Meta<'a> {
    running: bool,
    ready: bool,
    error: &'a Option<String>,
    permission: &'a Option<PermissionPrompt>,
    modes: &'a [(String, String)],
    mode: &'a Option<String>,
    models: &'a [(String, String)],
    model: &'a Option<String>,
    used: u64,
    size: u64,
    cost: f64,
    status_line: &'a Option<String>,
    turn_started: Option<i64>,
    policy: Policy,
}

fn meta_json(t: &Transcript, policy: Policy) -> String {
    let used = if t.usage.used > 0 {
        t.usage.used
    } else {
        t.estimate_tokens()
    };
    serde_json::to_string(&Meta {
        running: t.running,
        ready: t.ready,
        error: &t.error,
        permission: &t.permission,
        modes: &t.modes,
        mode: &t.mode,
        models: &t.models,
        model: &t.model,
        used,
        size: if t.usage.size > 0 {
            t.usage.size
        } else {
            200_000
        },
        cost: t.usage.cost,
        status_line: &t.status_line,
        turn_started: t.turn_started,
        policy,
    })
    .unwrap_or_default()
}

fn status_of(t: &Transcript) -> Status {
    Status {
        running: t.running,
        attention: t.permission.is_some(),
        error: t.error.is_some(),
    }
}

pub struct Hub {
    store: Store,
    clients: Mutex<HashMap<u64, Client>>,
    threads: Mutex<HashMap<i64, Arc<Live>>>,
    terms: Mutex<HashMap<u64, Arc<WebTerm>>>,
    term_subs: Mutex<HashMap<u64, HashSet<u64>>>,
    brains: Mutex<HashMap<PathBuf, Arc<(Brain, Graph)>>>,
    projects: Mutex<(Option<Instant>, Vec<Project>)>,
    dirty: Mutex<HashSet<i64>>,
    wake: flume::Sender<()>,
    next: AtomicU64,
    pub host: String,
}

impl Hub {
    pub fn new(store: Store) -> Arc<Self> {
        let (wake, rx) = flume::bounded::<()>(1);
        let hub = Arc::new(Self {
            store,
            clients: Mutex::default(),
            threads: Mutex::default(),
            terms: Mutex::default(),
            term_subs: Mutex::default(),
            brains: Mutex::default(),
            projects: Mutex::default(),
            dirty: Mutex::default(),
            wake,
            next: AtomicU64::new(1),
            host: host_name(),
        });
        let weak = Arc::downgrade(&hub);
        std::thread::Builder::new()
            .name("web-flush".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    std::thread::sleep(Duration::from_millis(33));
                    let Some(hub) = weak.upgrade() else { break };
                    let ids: Vec<i64> = hub.dirty.lock().drain().collect();
                    for id in ids {
                        hub.flush(id);
                    }
                }
            })
            .expect("spawn web flusher");
        hub
    }

    fn id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    // ---- clients ----

    pub fn connect(&self, tx: flume::Sender<Out>, sock: std::net::TcpStream) -> u64 {
        let id = self.id();
        self.clients.lock().insert(
            id,
            Client {
                tx,
                sock,
                threads: HashSet::new(),
            },
        );
        id
    }

    pub fn disconnect(&self, client: u64) {
        self.clients.lock().remove(&client);
        for subs in self.term_subs.lock().values_mut() {
            subs.remove(&client);
        }
    }

    pub fn client_count(&self) -> usize {
        self.clients.lock().len()
    }

    /// Close every connection (after the pairing token changes).
    pub fn disconnect_all(&self) {
        for c in self.clients.lock().values() {
            let _ = c.tx.try_send(Out::Close);
            c.kill();
        }
    }

    fn send_to(&self, client: u64, out: Out) {
        let clients = self.clients.lock();
        if let Some(c) = clients.get(&client)
            && c.tx.try_send(out).is_err()
        {
            // A client that can't keep up is dropped rather than buffered forever.
            c.kill();
        }
    }

    fn broadcast(&self, text: &str) {
        for c in self.clients.lock().values() {
            if c.tx.try_send(Out::Text(text.to_string())).is_err() {
                c.kill();
            }
        }
    }

    // ---- requests ----

    /// Handle one request; returns its result value.
    pub fn handle(self: &Arc<Self>, client: u64, method: &str, p: &Value) -> Result<Value> {
        let s = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
        let i = |k: &str| p.get(k).and_then(Value::as_i64);
        let path = |k: &str| {
            s(k).map(PathBuf::from)
                .ok_or_else(|| anyhow!("missing {k}"))
        };
        match method {
            "hello" => Ok(self.hello()),
            "projects" => {
                Ok(self.projects(p.get("refresh").and_then(Value::as_bool).unwrap_or(false)))
            }
            "project.add" => {
                let row = Project::probe(&path("path")?)?;
                self.store.add_project(&row)?;
                *self.projects.lock() = (None, vec![]);
                self.broadcast(r#"{"ev":"projects"}"#);
                Ok(json!({ "path": row.path }))
            }
            "fs.list" => Ok(list_dirs(s("path").as_deref())),
            "thread.new" => self.thread_new(p),
            "thread.open" => self.thread_open(client, i("id").context("id")?),
            "thread.leave" => {
                if let Some(c) = self.clients.lock().get_mut(&client) {
                    c.threads.remove(&i("id").unwrap_or_default());
                }
                Ok(Value::Null)
            }
            "thread.send" => {
                let brain = p.get("brain").and_then(Value::as_bool).unwrap_or(true);
                self.thread_send(i("id").context("id")?, s("text").unwrap_or_default(), brain)
            }
            "thread.cancel" => {
                let live = self.live(i("id").context("id")?)?;
                if let Some(sn) = &*live.session.lock() {
                    sn.cancel();
                }
                Ok(Value::Null)
            }
            "thread.answer" => {
                let live = self.live(i("id").context("id")?)?;
                if let Some(sn) = &*live.session.lock() {
                    sn.answer(s("option"));
                }
                self.mark(live.id);
                Ok(Value::Null)
            }
            "thread.model" | "thread.mode" => {
                let live = self.live(i("id").context("id")?)?;
                let v = s("value").context("value")?;
                self.ensure_session(&live);
                if let Some(sn) = &*live.session.lock() {
                    if method == "thread.model" {
                        sn.set_model(v);
                    } else {
                        sn.set_mode(v);
                    }
                }
                Ok(Value::Null)
            }
            "thread.policy" => {
                let live = self.live(i("id").context("id")?)?;
                let pol: Policy =
                    serde_json::from_value(p.get("value").cloned().unwrap_or_default())?;
                *live.policy.lock() = pol;
                if let Some(sn) = &*live.session.lock() {
                    *sn.policy.lock() = pol;
                }
                self.mark(live.id);
                Ok(Value::Null)
            }
            "thread.rename" => {
                let id = i("id").context("id")?;
                let title = s("title").unwrap_or_default();
                let title = title.trim();
                if title.is_empty() {
                    bail!("empty title");
                }
                self.store.update_session(id, Some(title), None, None, None);
                self.broadcast(r#"{"ev":"projects"}"#);
                Ok(Value::Null)
            }
            "thread.archive" => {
                let id = i("id").context("id")?;
                self.store.close_session(id);
                self.threads.lock().remove(&id);
                self.broadcast(r#"{"ev":"projects"}"#);
                Ok(Value::Null)
            }
            "git.status" => Ok(self.git_status(&path("worktree")?)),
            "git.diff" => {
                let wt = path("worktree")?;
                let base = self.base_of(&wt);
                let patch = git::file_patch(&wt, &base, &s("path").context("path")?);
                let lines: Vec<Value> = git::annotate_patch(&patch)
                    .into_iter()
                    .map(|l| json!([l.text, l.old, l.new]))
                    .collect();
                Ok(json!({ "lines": lines }))
            }
            "git.commit" => {
                let wt = path("worktree")?;
                let msg = s("message").unwrap_or_default();
                if msg.trim().is_empty() {
                    bail!("commit message is empty");
                }
                git::run(&wt, &["add", "-A"])?;
                git::run(&wt, &["commit", "-m", msg.trim()])?;
                self.touch_projects();
                Ok(self.git_status(&wt))
            }
            "git.push" => {
                let wt = path("worktree")?;
                let branch = git::current_branch(&wt).context("detached HEAD")?;
                git::run(&wt, &["push", "-u", "origin", &branch])?;
                Ok(self.git_status(&wt))
            }
            "git.pr" => {
                let wt = path("worktree")?;
                let base = self.base_of(&wt);
                let url = forge::create_pr(&wt, &base, settings::get().pr_draft)?;
                Ok(json!({ "url": url }))
            }
            "term.list" => {
                let wt = path("worktree")?;
                let terms = self.terms.lock();
                let mut v: Vec<&Arc<WebTerm>> =
                    terms.values().filter(|t| t.worktree == wt).collect();
                v.sort_by_key(|t| t.id);
                Ok(Value::Array(
                    v.iter()
                        .map(|t| json!({ "id": t.id, "title": t.title, "alive": t.alive.load(Ordering::Relaxed) }))
                        .collect(),
                ))
            }
            "term.open" => {
                let wt = path("worktree")?;
                let cols = i("cols").unwrap_or(100) as u16;
                let rows = i("rows").unwrap_or(30) as u16;
                let program = match s("command").filter(|c| !c.trim().is_empty()) {
                    Some(c) => Program::Command(c),
                    None => Program::Shell,
                };
                // The client follows up with `term.attach` for output.
                let t = self.open_term(&wt, program, cols, rows)?;
                Ok(json!({ "id": t.id, "title": t.title }))
            }
            "term.attach" => {
                let id = i("id").context("id")? as u64;
                let t = self
                    .terms
                    .lock()
                    .get(&id)
                    .cloned()
                    .context("terminal is gone")?;
                t.with_replay(|backlog| {
                    self.term_subs.lock().entry(id).or_default().insert(client);
                    if !backlog.is_empty() {
                        self.send_to(client, Out::Binary(term_frame(id, backlog)));
                    }
                });
                Ok(json!({ "id": id, "title": t.title, "alive": t.alive.load(Ordering::Relaxed) }))
            }
            "term.detach" => {
                let id = i("id").context("id")? as u64;
                if let Some(s) = self.term_subs.lock().get_mut(&id) {
                    s.remove(&client);
                }
                Ok(Value::Null)
            }
            "term.input" => {
                let id = i("id").context("id")? as u64;
                if let Some(t) = self.terms.lock().get(&id).cloned() {
                    t.write(s("data").unwrap_or_default().as_bytes());
                }
                Ok(Value::Null)
            }
            "term.resize" => {
                let id = i("id").context("id")? as u64;
                if let Some(t) = self.terms.lock().get(&id).cloned() {
                    t.resize(
                        i("cols").unwrap_or(80) as u16,
                        i("rows").unwrap_or(24) as u16,
                    );
                }
                Ok(Value::Null)
            }
            "term.close" => {
                let id = i("id").context("id")? as u64;
                self.terms.lock().remove(&id);
                self.term_subs.lock().remove(&id);
                self.broadcast(&json!({ "ev": "terms" }).to_string());
                Ok(Value::Null)
            }
            "script" => {
                let wt = path("worktree")?;
                Ok(match crate::project::run_command(&wt) {
                    Some((label, cmd)) => json!({ "label": label, "command": cmd }),
                    None => Value::Null,
                })
            }
            other => bail!("unknown method {other}"),
        }
    }

    /// Whether a method only touches memory (handled inline, in order).
    pub fn is_quick(method: &str) -> bool {
        matches!(
            method,
            "term.input"
                | "term.resize"
                | "term.detach"
                | "thread.leave"
                | "thread.cancel"
                | "thread.answer"
        )
    }

    fn hello(&self) -> Value {
        let s = settings::get();
        let agents: Vec<Value> = AGENTS
            .iter()
            .filter(|a| a.acp.is_some() && a.key != "super")
            .map(|a| {
                let override_ = s.agent_command(a.key).is_some();
                json!({
                    "key": a.key,
                    "name": a.name,
                    "mono": a.mono,
                    "available": override_ || a.acp.as_ref().is_some_and(AgentSpec::available),
                })
            })
            .collect();
        json!({
            "host": self.host,
            "version": env!("CARGO_PKG_VERSION"),
            "agents": agents,
            "default_agent": s.default_agent,
            "policy": crate::agents::acp::Policy::from_setting(s.approval),
            "theme": s.theme,
            "accent": s.accent,
            "brain_by_default": s.brain_by_default,
            "home": dirs::home_dir(),
        })
    }

    // ---- projects ----

    fn touch_projects(&self) {
        self.projects.lock().0 = None;
    }

    /// Projects with worktrees and their threads. Worktree scans (git) are
    /// cached for a few seconds; thread lists are always fresh.
    fn projects(&self, refresh: bool) -> Value {
        let stale = {
            let p = self.projects.lock();
            refresh || p.0.is_none_or(|t| t.elapsed() > Duration::from_secs(8))
        };
        if stale {
            let rows = self.store.projects().unwrap_or_default();
            let mut list: Vec<Project> = rows.iter().map(Project::from_row).collect();
            for p in &mut list {
                p.scan(&HashMap::new());
            }
            *self.projects.lock() = (Some(Instant::now()), list);
        }
        let live = self.threads.lock();
        let p = self.projects.lock();
        let projects: Vec<Value> =
            p.1.iter()
                .map(|pr| {
                    let worktrees: Vec<Value> = pr
                    .worktrees
                    .iter()
                    .map(|w| {
                        let threads: Vec<Value> = self
                            .store
                            .threads(&w.path, 60)
                            .into_iter()
                            .map(|t| {
                                let st = live
                                    .get(&t.id)
                                    .map(|l| status_of(&l.transcript.lock()))
                                    .unwrap_or_default();
                                json!({
                                    "id": t.id, "agent": t.agent, "title": t.title,
                                    "updated": t.updated, "status": st,
                                })
                            })
                            .collect();
                        json!({
                            "path": w.path, "branch": w.branch, "primary": w.primary,
                            "added": w.stat.added, "removed": w.stat.removed, "files": w.stat.files,
                            "last_commit": w.last_commit, "threads": threads,
                        })
                    })
                    .collect();
                    json!({
                        "path": pr.root, "name": pr.name, "base": pr.base, "stack": pr.stack,
                        "remote": remote::is_remote(&pr.root), "worktrees": worktrees,
                    })
                })
                .collect();
        Value::Array(projects)
    }

    fn project_of(&self, worktree: &Path) -> Option<(PathBuf, String, String)> {
        {
            let p = self.projects.lock();
            for pr in &p.1 {
                if pr.worktrees.iter().any(|w| w.path == worktree) || pr.root == worktree {
                    return Some((pr.root.clone(), pr.base.clone(), pr.name.clone()));
                }
            }
        }
        let rows = self.store.projects().ok()?;
        let root = git::repo_root(worktree).ok();
        rows.into_iter()
            .find(|r| Some(&r.path) == root.as_ref() || worktree.starts_with(&r.path))
            .map(|r| (r.path, r.base, r.name))
    }

    fn base_of(&self, worktree: &Path) -> String {
        self.project_of(worktree)
            .map(|p| p.1)
            .unwrap_or_else(|| git::default_branch(worktree))
    }

    fn git_status(&self, wt: &Path) -> Value {
        let base = self.base_of(wt);
        let files = git::changed_files(wt, &base);
        let stat = git::diff_stat(&files);
        let (ahead, behind) = git::ahead_behind(wt).unwrap_or((0, 0));
        let dirty =
            git::try_run(wt, &["status", "--porcelain"]).is_some_and(|s| !s.trim().is_empty());
        json!({
            "branch": git::current_branch(wt),
            "base": base,
            "ahead": ahead,
            "behind": behind,
            "dirty": dirty,
            "conflicts": git::has_conflicts(wt),
            "added": stat.added,
            "removed": stat.removed,
            "files": files.iter().map(|f| json!({
                "path": f.path, "added": f.added, "removed": f.removed, "binary": f.binary,
            })).collect::<Vec<_>>(),
        })
    }

    // ---- threads ----

    fn thread_new(self: &Arc<Self>, p: &Value) -> Result<Value> {
        let agent = p.get("agent").and_then(Value::as_str).unwrap_or("claude");
        let spec = AgentSpec::by_key(agent)
            .filter(|s| s.acp.is_some())
            .ok_or_else(|| anyhow!("unknown agent {agent}"))?;
        let project = PathBuf::from(
            p.get("project")
                .and_then(Value::as_str)
                .context("project")?,
        );
        let title = p
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let worktree = match p.get("worktree").and_then(Value::as_str) {
            // A fresh worktree on a new branch (T3-style "New worktree").
            Some("new") => {
                let (root, base, _) = self.project_of(&project).context("unknown project")?;
                let s = settings::get();
                let name = if title.is_empty() {
                    format!("task-{}", now() % 100_000)
                } else {
                    title.clone()
                };
                let branch = git::slugify_branch_with(&name, &s.branch_prefix);
                let path = git::add_worktree(&root, &branch, &base)?;
                for f in s.copy_list() {
                    if let Ok(bytes) = remote::read_file(&root.join(&f)) {
                        let _ = remote::write_file(&path.join(&f), &bytes);
                    }
                }
                let setup = s.setup_command.trim().to_string();
                if !setup.is_empty() {
                    let _ = self.open_term(&path, Program::Command(setup), 100, 30);
                }
                self.touch_projects();
                path
            }
            Some(w) => PathBuf::from(w),
            None => project.clone(),
        };
        let title = if title.is_empty() {
            "New thread".to_string()
        } else {
            title
        };
        let id = self.store.create_session(&worktree, spec.key, &title)?;
        self.broadcast(r#"{"ev":"projects"}"#);
        Ok(json!({ "id": id, "worktree": worktree }))
    }

    fn live(self: &Arc<Self>, id: i64) -> Result<Arc<Live>> {
        if let Some(l) = self.threads.lock().get(&id) {
            return Ok(l.clone());
        }
        let row = self.store.session(id).context("no such thread")?;
        let spec = AgentSpec::by_key(&row.agent).context("unknown agent")?;
        let mut t = Transcript::from_history(self.store.events::<Item>(id));
        t.usage.used = row.tokens.max(0) as u64;
        t.usage.cost = row.cost;
        let sent_first = !t.items.is_empty();
        let project = self
            .project_of(&row.worktree)
            .map(|p| p.2)
            .unwrap_or_default();
        let live = Arc::new(Live {
            id,
            worktree: row.worktree,
            project,
            spec,
            transcript: Arc::new(Mutex::new(t)),
            session: Mutex::new(None),
            acp_id: Mutex::new(row.acp_id),
            policy: Mutex::new(Policy::from_setting(settings::get().approval)),
            sent_first: AtomicBool::new(sent_first),
            sent: Mutex::default(),
        });
        Ok(self.threads.lock().entry(id).or_insert(live).clone())
    }

    fn thread_open(self: &Arc<Self>, client: u64, id: i64) -> Result<Value> {
        let live = self.live(id)?;
        if let Some(c) = self.clients.lock().get_mut(&client) {
            c.threads.insert(id);
        }
        let row = self.store.session(id).context("no such thread")?;
        let t = live.transcript.lock();
        let items = serde_json::to_value(&t.items)?;
        let meta: Value = serde_json::from_str(&meta_json(&t, *live.policy.lock()))?;
        Ok(json!({
            "id": id,
            "title": row.title,
            "agent": live.spec.key,
            "worktree": live.worktree,
            "project": live.project,
            "branch": git::current_branch(&live.worktree),
            "items": items,
            "meta": meta,
        }))
    }

    fn ensure_session(self: &Arc<Self>, live: &Arc<Live>) {
        let mut session = live.session.lock();
        if session.is_some() {
            return;
        }
        let Some(cmd) = live.spec.acp else { return };
        let launch = settings::get()
            .agent_command(live.spec.key)
            .map(|(program, args)| Launch { program, args })
            .unwrap_or_else(|| cmd.into());
        let weak = Arc::downgrade(self);
        let id = live.id;
        let notify: crate::agents::acp::Notify = Arc::new(move || {
            if let Some(h) = weak.upgrade() {
                h.mark(id);
            }
        });
        *session = Some(AcpSession::start(
            launch,
            live.worktree.clone(),
            live.acp_id.lock().take(),
            live.transcript.clone(),
            Some((self.store.clone(), live.id)),
            *live.policy.lock(),
            notify,
        ));
    }

    fn thread_send(self: &Arc<Self>, id: i64, text: String, use_brain: bool) -> Result<Value> {
        let text = text.trim().to_string();
        if text.is_empty() {
            bail!("empty message");
        }
        let live = self.live(id)?;
        self.ensure_session(&live);
        let mut blocks = Vec::new();
        if !live.sent_first.swap(true, Ordering::Relaxed) {
            if use_brain
                && let Some(digest) = self.brain_digest(&live.worktree, &live.project, &text)
            {
                blocks.push(Block::Context {
                    uri: format!("insyde://brain/{}", live.project),
                    text: digest,
                });
            }
            let first = text
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim();
            let title: String = first.chars().take(60).collect();
            self.store
                .update_session(id, Some(&title), None, None, None);
            self.broadcast(r#"{"ev":"projects"}"#);
        }
        blocks.push(Block::Text(text));
        {
            // Show "working" immediately; the agent confirms when the turn starts.
            let mut t = live.transcript.lock();
            t.running = true;
            t.error = None;
            t.turn_started.get_or_insert(now());
        }
        if let Some(sn) = &*live.session.lock() {
            sn.prompt(blocks);
        }
        self.mark(id);
        Ok(Value::Null)
    }

    fn brain_digest(&self, worktree: &Path, project: &str, task: &str) -> Option<String> {
        let (root, base, _) = self.project_of(worktree)?;
        let handle = {
            let cached = self.brains.lock().get(&root).cloned();
            match cached {
                Some(h) => h,
                None => {
                    if !Brain::exists(&root) {
                        return None;
                    }
                    let brain = Brain::open(&root, &base).ok()?;
                    let graph = brain.load().ok()?;
                    let h = Arc::new((brain, graph));
                    self.brains.lock().insert(root.clone(), h.clone());
                    h
                }
            }
        };
        let budget = settings::get().brain_budget_tokens.clamp(2_000, 100_000) as usize;
        let d = handle.0.digest(&handle.1, project, task, budget);
        (!d.is_empty()).then_some(d)
    }

    fn mark(&self, id: i64) {
        self.dirty.lock().insert(id);
        let _ = self.wake.try_send(());
    }

    /// Send what changed in thread `id` since the last flush.
    fn flush(&self, id: i64) {
        let Some(live) = self.threads.lock().get(&id).cloned() else {
            return;
        };
        let policy = *live.policy.lock();
        let mut sent = live.sent.lock();
        let (items, meta, status) = {
            let t = live.transcript.lock();
            let status = status_of(&t);
            // A finished turn re-checks everything; streaming only the tail.
            let from = if status.running {
                sent.0.len().saturating_sub(TAIL).min(t.items.len())
            } else {
                0
            };
            let tail: Vec<String> = t.items[from..]
                .iter()
                .map(|i| serde_json::to_string(i).unwrap_or_default())
                .collect();
            let mut items: Vec<String> = sent.0[..from.min(sent.0.len())].to_vec();
            items.extend(tail);
            (items, meta_json(&t, policy), status)
        };
        let mut set = String::new();
        for (i, j) in items.iter().enumerate() {
            if sent.0.get(i) != Some(j) {
                if !set.is_empty() {
                    set.push(',');
                }
                set.push_str(&format!("[{i},{j}]"));
            }
        }
        let changed = !set.is_empty() || items.len() != sent.0.len() || meta != sent.1;
        let status_changed = status != sent.2;
        if changed {
            let msg = format!(
                r#"{{"ev":"thread","id":{id},"len":{},"set":[{set}],"meta":{meta}}}"#,
                items.len()
            );
            let clients = self.clients.lock();
            for c in clients.values().filter(|c| c.threads.contains(&id)) {
                let _ = c.tx.try_send(Out::Text(msg.clone()));
            }
        }
        *sent = (items, meta, status);
        drop(sent);
        if status_changed {
            self.broadcast(&json!({ "ev": "status", "id": id, "status": status }).to_string());
            if !status.running {
                self.touch_projects();
            }
        }
    }

    // ---- terminals ----

    fn open_term(
        self: &Arc<Self>,
        wt: &Path,
        program: Program,
        cols: u16,
        rows: u16,
    ) -> Result<Arc<WebTerm>> {
        let id = self.id();
        let (w1, w2) = (Arc::downgrade(self), Arc::downgrade(self));
        let t = WebTerm::spawn(
            id,
            wt,
            program,
            cols,
            rows,
            move |bytes| {
                if let Some(h) = w1.upgrade() {
                    let subs: Vec<u64> = h
                        .term_subs
                        .lock()
                        .get(&id)
                        .map(|s| s.iter().copied().collect())
                        .unwrap_or_default();
                    if subs.is_empty() {
                        return;
                    }
                    let frame = term_frame(id, bytes);
                    for c in subs {
                        h.send_to(c, Out::Binary(frame.clone()));
                    }
                }
            },
            move || {
                if let Some(h) = w2.upgrade() {
                    h.broadcast(&json!({ "ev": "term.exit", "id": id }).to_string());
                }
            },
        )?;
        self.terms.lock().insert(id, t.clone());
        self.broadcast(&json!({ "ev": "terms" }).to_string());
        Ok(t)
    }
}

fn term_frame(id: u64, bytes: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(bytes.len() + 5);
    f.push(BIN_TERM);
    f.extend_from_slice(&(id as u32).to_be_bytes());
    f.extend_from_slice(bytes);
    f
}

/// Folders under `path` (for the "Add project" picker), git repos flagged.
fn list_dirs(path: Option<&str>) -> Value {
    let base = path
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| "/".into());
    let mut dirs: Vec<Value> = remote::list_dir(&base)
        .into_iter()
        .filter(|(_, d)| *d)
        .take(400)
        .map(|(name, _)| {
            let p = base.join(&name);
            let repo = !remote::is_remote(&p) && p.join(".git").exists();
            json!({ "name": name, "path": p, "repo": repo })
        })
        .collect();
    dirs.sort_by_key(|d| !d["repo"].as_bool().unwrap_or(false));
    json!({ "path": base, "parent": base.parent(), "dirs": dirs })
}

fn host_name() -> String {
    std::process::Command::new("hostname")
        .arg("-s")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "this machine".into())
}
