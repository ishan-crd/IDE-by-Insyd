//! ACP client: drives one agent subprocess for one chat session.
//!
//! Each session runs on its own small thread with a `smol` executor (the ACP
//! SDK is runtime-agnostic and uses `async-process`). The UI talks to it
//! through a command channel and reads a shared [`Transcript`]; a `notify`
//! callback tells the UI to repaint. Nothing here blocks the UI thread.
//!
//! Threading rule from the ACP SDK: request handlers hold the dispatch loop,
//! so a permission prompt that waits for the user is moved into a spawned
//! task and the handler returns immediately.

use super::transcript::{
    self, Item, PermissionPrompt, PlanEntry, ToolItem, ToolKind, ToolStatus, Transcript,
};
use super::{Cmd, augmented_path_blocking, which};
use crate::store::{Store, now};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1 as acp;
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, ConnectionTo};
use futures::FutureExt;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A prompt content block.
#[derive(Clone, Debug)]
pub enum Block {
    Text(String),
    /// Extra context (brain digest, hand-off bundle) sent as an embedded
    /// resource so agents treat it as reference material, not instructions.
    Context {
        uri: String,
        text: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Policy {
    /// Ask for every non-read action.
    Ask,
    /// Auto-approve file edits inside the worktree; ask for commands.
    AcceptEdits,
    /// Approve everything.
    FullAccess,
}

impl Policy {
    pub fn label(self) -> &'static str {
        match self {
            Policy::Ask => "Ask every time",
            Policy::AcceptEdits => "Auto-approve edits",
            Policy::FullAccess => "Full access",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Policy::Ask => Policy::AcceptEdits,
            Policy::AcceptEdits => Policy::FullAccess,
            Policy::FullAccess => Policy::Ask,
        }
    }
}

enum Command {
    Prompt(Vec<Block>),
    Cancel,
    SetMode(String),
    SetModel(String),
    Permission(Option<String>),
    Shutdown,
}

pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// Handle to a running ACP session. Dropping it shuts the agent down.
pub struct AcpSession {
    pub transcript: Arc<Mutex<Transcript>>,
    pub policy: Arc<Mutex<Policy>>,
    tx: flume::Sender<Command>,
}

impl AcpSession {
    /// Start the agent process. `resume` is the agent's own session id from a
    /// previous run; `transcript` may already hold that session's history
    /// (loaded from the store), which is shown while the agent starts.
    pub fn start(
        cmd: Cmd,
        cwd: PathBuf,
        resume: Option<String>,
        transcript: Arc<Mutex<Transcript>>,
        persist: Option<(Store, i64)>,
        policy: Policy,
        notify: Notify,
    ) -> Self {
        {
            let mut t = transcript.lock();
            t.persisted = t.persisted.max(t.items.len());
        }
        let policy = Arc::new(Mutex::new(policy));
        let (tx, rx) = flume::unbounded();
        let ctx = Ctx {
            transcript: transcript.clone(),
            policy: policy.clone(),
            notify,
            cwd,
            persist,
            pending: Arc::new(Mutex::new(None)),
        };
        std::thread::Builder::new()
            .name("acp-session".into())
            .spawn(move || {
                let err_ctx = ctx.clone();
                if let Err(e) = smol::block_on(run(cmd, resume, ctx, rx)) {
                    let mut t = err_ctx.transcript.lock();
                    t.running = false;
                    t.error = Some(explain_error(&e.to_string(), cmd));
                    t.touch();
                    drop(t);
                    (err_ctx.notify)();
                }
            })
            .expect("spawn acp thread");
        Self {
            transcript,
            policy,
            tx,
        }
    }

    pub fn prompt(&self, blocks: Vec<Block>) {
        if let Some(Block::Text(text)) = blocks.iter().find(|b| matches!(b, Block::Text(_))) {
            let mut t = self.transcript.lock();
            t.items.push(Item::User { text: text.clone() });
            t.touch();
        }
        let _ = self.tx.send(Command::Prompt(blocks));
    }
    pub fn cancel(&self) {
        let _ = self.tx.send(Command::Cancel);
    }
    pub fn set_mode(&self, id: String) {
        let _ = self.tx.send(Command::SetMode(id));
    }
    pub fn set_model(&self, id: String) {
        let _ = self.tx.send(Command::SetModel(id));
    }
    /// Answer the pending permission prompt (`None` = reject/cancel).
    pub fn answer(&self, option_id: Option<String>) {
        self.transcript.lock().permission = None;
        let _ = self.tx.send(Command::Permission(option_id));
    }
}

impl Drop for AcpSession {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
    }
}

#[derive(Clone)]
struct Ctx {
    transcript: Arc<Mutex<Transcript>>,
    policy: Arc<Mutex<Policy>>,
    notify: Notify,
    cwd: PathBuf,
    persist: Option<(Store, i64)>,
    pending: Arc<Mutex<Option<flume::Sender<Option<String>>>>>,
}

impl Ctx {
    fn update(&self, f: impl FnOnce(&mut Transcript)) {
        {
            let mut t = self.transcript.lock();
            f(&mut t);
            t.touch();
        }
        (self.notify)();
    }

    /// Write finished items to the store.
    fn flush(&self) {
        let Some((store, id)) = &self.persist else {
            return;
        };
        let mut t = self.transcript.lock();
        let from = t.persisted.min(t.items.len());
        for (i, it) in t.items.iter().enumerate().skip(from) {
            store.append_event(*id, i as i64, it);
        }
        t.persisted = t.items.len();
        store.update_session(
            *id,
            None,
            None,
            Some(t.usage.used as i64),
            Some(t.usage.cost),
        );
    }
}

fn explain_error(e: &str, cmd: Cmd) -> String {
    if e.contains("No such file") || e.contains("not found") || e.contains("os error 2") {
        format!(
            "Couldn't start `{}`. Install it (or Node.js for npx-based agents) and try again.",
            cmd.program
        )
    } else if e.to_lowercase().contains("auth") {
        format!("The agent needs you to log in. Run its CLI once in a terminal to sign in.\n\n{e}")
    } else {
        e.to_string()
    }
}

fn rel(cwd: &Path, p: &Path) -> String {
    p.strip_prefix(cwd)
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

/// True when a shell command consists only of `insy …` invocations (joined by
/// `;`, `&&`, `||` or newlines) with no pipes, redirects or substitutions.
pub fn is_insy_only(cmd: &str) -> bool {
    let cmd = cmd.trim();
    if cmd.is_empty()
        || cmd.contains('|') && !cmd.contains("||")
        || ['`', '>', '<'].iter().any(|c| cmd.contains(*c))
        || cmd.contains("$(")
    {
        return false;
    }
    cmd.split(['\n', ';'])
        .flat_map(|s| s.split("&&"))
        .flat_map(|s| s.split("||"))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .all(|s| s == "insy" || s.starts_with("insy ") && !s.contains('|') && !s.contains('&'))
}

fn map_kind(k: &acp::ToolKind) -> ToolKind {
    match k {
        acp::ToolKind::Read => ToolKind::Read,
        acp::ToolKind::Edit => ToolKind::Edit,
        acp::ToolKind::Delete => ToolKind::Delete,
        acp::ToolKind::Move => ToolKind::Move,
        acp::ToolKind::Search => ToolKind::Search,
        acp::ToolKind::Execute => ToolKind::Bash,
        acp::ToolKind::Think => ToolKind::Think,
        acp::ToolKind::Fetch => ToolKind::Fetch,
        _ => ToolKind::Other,
    }
}

fn map_status(s: &acp::ToolCallStatus) -> ToolStatus {
    match s {
        acp::ToolCallStatus::Pending => ToolStatus::Pending,
        acp::ToolCallStatus::InProgress => ToolStatus::Running,
        acp::ToolCallStatus::Completed => ToolStatus::Done,
        acp::ToolCallStatus::Failed => ToolStatus::Failed,
        _ => ToolStatus::Running,
    }
}

/// Best "argument" to show for a tool call: a command, a path, or the title.
fn tool_arg(
    cwd: &Path,
    title: &str,
    raw: Option<&serde_json::Value>,
    locs: &[acp::ToolCallLocation],
) -> String {
    if let Some(v) = raw {
        for k in ["command", "cmd"] {
            if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
                return s.lines().next().unwrap_or(s).to_string();
            }
            if let Some(a) = v.get(k).and_then(|x| x.as_array()) {
                return a
                    .iter()
                    .filter_map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
            }
        }
        for k in ["file_path", "path", "filePath", "notebook_path"] {
            if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
                return rel(cwd, Path::new(s));
            }
        }
        for k in ["pattern", "query", "url"] {
            if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
                return s.to_string();
            }
        }
    }
    if let Some(l) = locs.first() {
        return rel(cwd, &l.path);
    }
    title.to_string()
}

fn apply_content(t: &mut ToolItem, cwd: &Path, content: &[acp::ToolCallContent]) {
    let (mut a, mut r) = (0, 0);
    let mut had_diff = false;
    let mut text_lines = 0usize;
    for c in content {
        match c {
            acp::ToolCallContent::Diff(d) => {
                had_diff = true;
                let (x, y) =
                    transcript::diff_counts(d.old_text.as_deref().unwrap_or(""), &d.new_text);
                a += x;
                r += y;
                let p = rel(cwd, &d.path);
                if !t.paths.contains(&p) {
                    t.paths.push(p);
                }
            }
            acp::ToolCallContent::Content(c) => {
                if let acp::ContentBlock::Text(tx) = &c.content {
                    text_lines += tx.text.lines().count();
                }
            }
            _ => {}
        }
    }
    if had_diff {
        t.added = a;
        t.removed = r;
        t.meta = format!("+{a} −{r}");
    } else if t.kind == ToolKind::Read && text_lines > 0 {
        t.meta = format!("{text_lines} lines");
    }
}

fn finish_meta(t: &mut ToolItem) {
    if t.kind == ToolKind::Bash || t.kind == ToolKind::Search || t.kind == ToolKind::Fetch {
        let secs = t.ended.unwrap_or_else(now) - t.started;
        let dur = if secs >= 60 {
            format!("{}m {}s", secs / 60, secs % 60)
        } else {
            format!("{secs}s")
        };
        t.meta = match t.status {
            ToolStatus::Failed => format!("failed · {dur}"),
            _ => dur,
        };
    } else if t.status == ToolStatus::Failed && t.meta.is_empty() {
        t.meta = "failed".into();
    }
}

fn on_update(ctx: &Ctx, u: acp::SessionUpdate) {
    let cwd = ctx.cwd.clone();
    match u {
        acp::SessionUpdate::AgentMessageChunk(c) => {
            if let acp::ContentBlock::Text(t) = c.content {
                ctx.update(|tr| tr.push_text(false, &t.text));
            }
        }
        acp::SessionUpdate::AgentThoughtChunk(c) => {
            if let acp::ContentBlock::Text(t) = c.content {
                ctx.update(|tr| tr.push_text(true, &t.text));
            }
        }
        acp::SessionUpdate::ToolCall(tc) => ctx.update(|tr| {
            let mut item = ToolItem {
                id: tc.tool_call_id.0.to_string(),
                kind: map_kind(&tc.kind),
                arg: tool_arg(&cwd, &tc.title, tc.raw_input.as_ref(), &tc.locations),
                title: tc.title.clone(),
                status: map_status(&tc.status),
                meta: String::new(),
                added: 0,
                removed: 0,
                paths: tc.locations.iter().map(|l| rel(&cwd, &l.path)).collect(),
                started: now(),
                ended: None,
            };
            apply_content(&mut item, &cwd, &tc.content);
            if let Some(existing) = tr.tool_mut(&item.id) {
                *existing = item;
            } else {
                tr.push_tool(item);
            }
        }),
        acp::SessionUpdate::ToolCallUpdate(up) => ctx.update(|tr| {
            let id = up.tool_call_id.0.to_string();
            let Some(t) = tr.tool_mut(&id) else { return };
            let f = up.fields;
            if let Some(k) = f.kind.as_ref() {
                t.kind = map_kind(k);
            }
            if let Some(title) = f.title {
                t.title = title;
            }
            if f.raw_input.is_some() || f.locations.is_some() {
                let locs = f.locations.clone().unwrap_or_default();
                t.arg = tool_arg(&cwd, &t.title, f.raw_input.as_ref(), &locs);
                for l in &locs {
                    let p = rel(&cwd, &l.path);
                    if !t.paths.contains(&p) {
                        t.paths.push(p);
                    }
                }
            }
            if let Some(c) = f.content.as_ref() {
                apply_content(t, &cwd, c);
            }
            if let Some(s) = f.status.as_ref() {
                t.status = map_status(s);
                if matches!(t.status, ToolStatus::Done | ToolStatus::Failed) {
                    t.ended = Some(now());
                    finish_meta(t);
                }
            }
        }),
        acp::SessionUpdate::Plan(p) => ctx.update(|tr| {
            let entries: Vec<PlanEntry> = p
                .entries
                .iter()
                .map(|e| PlanEntry {
                    text: e.content.clone(),
                    done: matches!(e.status, acp::PlanEntryStatus::Completed),
                    active: matches!(e.status, acp::PlanEntryStatus::InProgress),
                })
                .collect();
            if let Some(Item::Plan { entries: old }) = tr
                .items
                .iter_mut()
                .rev()
                .find(|i| matches!(i, Item::Plan { .. }))
            {
                *old = entries;
            } else {
                tr.items.push(Item::Plan { entries });
            }
        }),
        acp::SessionUpdate::UsageUpdate(u) => ctx.update(|tr| {
            tr.usage.used = u.used;
            tr.usage.size = u.size;
            if let Some(c) = u.cost {
                tr.usage.cost = c.amount;
            }
        }),
        acp::SessionUpdate::AvailableCommandsUpdate(c) => ctx.update(|tr| {
            tr.commands = c
                .available_commands
                .into_iter()
                .map(|c| (c.name, c.description))
                .collect();
        }),
        acp::SessionUpdate::CurrentModeUpdate(m) => {
            ctx.update(|tr| tr.mode = Some(m.current_mode_id.0.to_string()))
        }
        acp::SessionUpdate::ConfigOptionUpdate(c) => {
            ctx.update(|tr| apply_config(tr, &c.config_options))
        }
        _ => {}
    }
}

fn apply_config(tr: &mut Transcript, opts: &[acp::SessionConfigOption]) {
    for o in opts {
        let is_model = matches!(o.category, Some(acp::SessionConfigOptionCategory::Model));
        if !is_model {
            continue;
        }
        if let acp::SessionConfigKind::Select(s) = &o.kind {
            tr.model_config_id = Some(o.id.0.to_string());
            tr.model = Some(s.current_value.0.to_string());
            tr.models = match &s.options {
                acp::SessionConfigSelectOptions::Ungrouped(v) => v
                    .iter()
                    .map(|x| (x.value.0.to_string(), x.name.clone()))
                    .collect(),
                acp::SessionConfigSelectOptions::Grouped(g) => g
                    .iter()
                    .flat_map(|g| {
                        g.options
                            .iter()
                            .map(|x| (x.value.0.to_string(), x.name.clone()))
                    })
                    .collect(),
                _ => vec![],
            };
        }
    }
}

fn to_acp(blocks: Vec<Block>) -> Vec<acp::ContentBlock> {
    blocks
        .into_iter()
        .map(|b| match b {
            Block::Text(t) => acp::ContentBlock::Text(acp::TextContent::new(t)),
            Block::Context { uri, text } => acp::ContentBlock::Resource(
                acp::EmbeddedResource::new(acp::EmbeddedResourceResource::TextResourceContents(
                    acp::TextResourceContents::new(text, uri)
                        .mime_type("text/markdown".to_string()),
                )),
            ),
        })
        .collect()
}

/// Paths an agent may write through `fs/write_text_file`: inside the worktree.
fn allowed(cwd: &Path, p: &Path) -> bool {
    let target = p
        .parent()
        .and_then(|d| d.canonicalize().ok())
        .unwrap_or_else(|| p.to_path_buf());
    let root = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    target.starts_with(&root)
}

async fn run(
    cmd: Cmd,
    resume: Option<String>,
    ctx: Ctx,
    rx: flume::Receiver<Command>,
) -> anyhow::Result<()> {
    let remote = crate::remote::split(&ctx.cwd);
    // Remote worktrees: run the agent on the host, speaking ACP over the SSH pipe.
    let (program, args): (PathBuf, Vec<String>) = match &remote {
        Some((host, dir)) => {
            let (p, a) = crate::remote::command_parts(
                host,
                &crate::remote::script_in(dir, cmd.program, cmd.args),
                false,
            );
            (PathBuf::from(p), a)
        }
        None => (
            which(cmd.program).ok_or_else(|| anyhow::anyhow!("No such file: {}", cmd.program))?,
            cmd.args.iter().map(|s| s.to_string()).collect(),
        ),
    };
    let session_cwd = remote
        .as_ref()
        .map(|(_, d)| PathBuf::from(d))
        .unwrap_or_else(|| ctx.cwd.clone());
    let mut config = AcpAgentConfig::new(program)
        .args(args)
        .env("PATH", augmented_path_blocking())
        // Let agents coordinate through the `insy` CLI.
        .env(
            "INSYDE_SOCKET",
            crate::rpc::socket_path().to_string_lossy().to_string(),
        )
        .env("INSYDE_WORKTREE", ctx.cwd.to_string_lossy().to_string());
    if let Some(h) = dirs::home_dir() {
        config = config.env("HOME", h.to_string_lossy().to_string());
    }
    let mut agent = AcpAgent::new(config);
    if std::env::var_os("INSYDE_ACP_DEBUG").is_some() {
        agent = agent.with_debug(|line, dir| {
            eprintln!(
                "[acp {dir:?}] {}",
                line.chars().take(400).collect::<String>()
            )
        });
    }

    let n_ctx = ctx.clone();
    let p_ctx = ctx.clone();
    let r_ctx = ctx.clone();
    let w_ctx = ctx.clone();
    let replaying = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let replay_flag = replaying.clone();

    agent_client_protocol::Client
        .builder()
        .on_receive_notification(
            async move |n: acp::SessionNotification, _cx| {
                if !replay_flag.load(std::sync::atomic::Ordering::Acquire) {
                    on_update(&n_ctx, n.update);
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: acp::RequestPermissionRequest, responder, connection| {
                let kind = req.tool_call.fields.kind.as_ref().map(map_kind);
                let policy = *p_ctx.policy.lock();
                let command = req.tool_call.fields.raw_input.as_ref().and_then(|v| v.get("command")).and_then(|c| c.as_str()).unwrap_or("");
                // Team coordination through `insy` is always allowed; nothing else is.
                let insy = matches!(kind, Some(ToolKind::Bash)) && is_insy_only(command);
                let auto = insy || match policy {
                    Policy::FullAccess => true,
                    Policy::AcceptEdits => matches!(kind, Some(ToolKind::Edit | ToolKind::Read | ToolKind::Search | ToolKind::Think)),
                    Policy::Ask => matches!(kind, Some(ToolKind::Read | ToolKind::Search | ToolKind::Think)),
                };
                let allow_once = req
                    .options
                    .iter()
                    .find(|o| matches!(o.kind, acp::PermissionOptionKind::AllowOnce))
                    .or_else(|| req.options.iter().find(|o| matches!(o.kind, acp::PermissionOptionKind::AllowAlways)))
                    .map(|o| o.option_id.clone());
                if auto
                    && let Some(id) = allow_once {
                        return responder.respond(acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Selected(
                            acp::SelectedPermissionOutcome::new(id),
                        )));
                    }
                let (tx, rx) = flume::bounded::<Option<String>>(1);
                *p_ctx.pending.lock() = Some(tx);
                let cwd = p_ctx.cwd.clone();
                let title = req.tool_call.fields.title.clone().unwrap_or_else(|| "Permission requested".into());
                let detail = tool_arg(&cwd, &title, req.tool_call.fields.raw_input.as_ref(), req.tool_call.fields.locations.as_deref().unwrap_or(&[]));
                let options = req
                    .options
                    .iter()
                    .map(|o| {
                        let allow = matches!(o.kind, acp::PermissionOptionKind::AllowOnce | acp::PermissionOptionKind::AllowAlways);
                        (o.option_id.0.to_string(), o.name.clone(), allow)
                    })
                    .collect();
                p_ctx.update(|t| t.permission = Some(PermissionPrompt { title, detail, options }));
                connection.spawn(async move {
                    let choice = rx.recv_async().await.ok().flatten();
                    let outcome = match choice {
                        Some(id) => acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(id)),
                        None => acp::RequestPermissionOutcome::Cancelled,
                    };
                    responder.respond(acp::RequestPermissionResponse::new(outcome))
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: acp::ReadTextFileRequest, responder, _cx| {
                match std::fs::read_to_string(&req.path) {
                    Ok(s) => {
                        let text = match (req.line, req.limit) {
                            (None, None) => s,
                            (line, limit) => {
                                let start = line.unwrap_or(1).saturating_sub(1) as usize;
                                let take = limit.map(|l| l as usize).unwrap_or(usize::MAX);
                                s.lines().skip(start).take(take).collect::<Vec<_>>().join("\n")
                            }
                        };
                        let _ = &r_ctx;
                        responder.respond(acp::ReadTextFileResponse::new(text))
                    }
                    Err(e) => responder.respond_with_error(agent_client_protocol::Error::internal_error().data(e.to_string())),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: acp::WriteTextFileRequest, responder, _cx| {
                if !allowed(&w_ctx.cwd, &req.path) {
                    return responder.respond_with_error(
                        agent_client_protocol::Error::invalid_params().data(format!("{} is outside the worktree", req.path.display())),
                    );
                }
                if let Some(d) = req.path.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                match std::fs::write(&req.path, req.content.as_bytes()) {
                    Ok(()) => responder.respond(acp::WriteTextFileResponse::new()),
                    Err(e) => responder.respond_with_error(agent_client_protocol::Error::internal_error().data(e.to_string())),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |conn: ConnectionTo<Agent>| async move {
            let caps = acp::ClientCapabilities::new()
                // Remote agents use their own file tools on the host.
                .fs(acp::FileSystemCapabilities::new().read_text_file(remote.is_none()).write_text_file(remote.is_none()))
                .terminal(false);
            let init = conn
                .send_request(
                    acp::InitializeRequest::new(ProtocolVersion::V1)
                        .client_capabilities(caps)
                        .client_info(acp::Implementation::new("insyde", env!("CARGO_PKG_VERSION")).title("InsyDE".to_string())),
                )
                .block_task()
                .await?;

            // Load the previous session if the agent supports it, else start fresh.
            let mut session_id: Option<acp::SessionId> = None;
            if let (Some(id), true) = (resume.clone(), init.agent_capabilities.load_session) {
                replaying.store(true, std::sync::atomic::Ordering::Release);
                let r = conn.send_request(acp::LoadSessionRequest::new(id.clone(), session_cwd.clone())).block_task().await;
                replaying.store(false, std::sync::atomic::Ordering::Release);
                if let Ok(resp) = r {
                    session_id = Some(acp::SessionId::new(id));
                    ctx.update(|t| {
                        if let Some(m) = resp.modes {
                            t.mode = Some(m.current_mode_id.0.to_string());
                            t.modes = m.available_modes.into_iter().map(|m| (m.id.0.to_string(), m.name)).collect();
                        }
                        if let Some(c) = resp.config_options.as_ref() {
                            apply_config(t, c);
                        }
                    });
                }
            }
            let sid = match session_id {
                Some(s) => s,
                None => {
                    let resp = conn.send_request(acp::NewSessionRequest::new(session_cwd.clone())).block_task().await?;
                    if let Some((store, id)) = &ctx.persist {
                        store.update_session(*id, None, Some(&resp.session_id.0), None, None);
                    }
                    ctx.update(|t| {
                        if let Some(m) = resp.modes {
                            t.mode = Some(m.current_mode_id.0.to_string());
                            t.modes = m.available_modes.into_iter().map(|m| (m.id.0.to_string(), m.name)).collect();
                        }
                        if let Some(c) = resp.config_options.as_ref() {
                            apply_config(t, c);
                        }
                    });
                    resp.session_id
                }
            };
            ctx.update(|t| t.ready = true);

            let mut queue: std::collections::VecDeque<Vec<Block>> = Default::default();
            loop {
                let cmd = match queue.pop_front() {
                    Some(b) => Command::Prompt(b),
                    None => match rx.recv_async().await {
                        Ok(c) => c,
                        Err(_) => break,
                    },
                };
                match cmd {
                    Command::Prompt(blocks) => {
                        let started = now();
                        let mut worked_ix = 0;
                        ctx.update(|t| {
                            t.running = true;
                            t.turn_started = Some(started);
                            t.error = None;
                            t.items.push(Item::Worked { secs: 0 });
                            worked_ix = t.items.len() - 1;
                        });
                        ctx.flush();
                        let mut fut = Box::pin(conn.send_request(acp::PromptRequest::new(sid.clone(), to_acp(blocks))).block_task().fuse());
                        let result = loop {
                            let mut next = Box::pin(rx.recv_async().fuse());
                            futures::select! {
                                r = fut => break r,
                                c = next => match c {
                                    Ok(Command::Cancel) => { let _ = conn.send_notification(acp::CancelNotification::new(sid.clone())); }
                                    Ok(Command::Permission(choice)) => { if let Some(tx) = ctx.pending.lock().take() { let _ = tx.send(choice); } }
                                    Ok(Command::SetMode(m)) => spawn_set_mode(&conn, &sid, m, &ctx),
                                    Ok(Command::SetModel(m)) => spawn_set_model(&conn, &sid, m, &ctx),
                                    Ok(Command::Prompt(b)) => queue.push_back(b),
                                    Ok(Command::Shutdown) | Err(_) => return Ok(()),
                                },
                            }
                        };
                        let secs = now() - started;
                        ctx.update(|t| {
                            t.running = false;
                            t.permission = None;
                            // Record the turn's duration on its "Worked for" marker.
                            if let Some(Item::Worked { secs: s }) = t.items.iter_mut().rev().find(|i| matches!(i, Item::Worked { .. })) {
                                *s = secs;
                            }
                            match &result {
                                Ok(r) if matches!(r.stop_reason, acp::StopReason::Cancelled) => {
                                    t.items.push(Item::Notice { text: "Stopped.".into(), error: false })
                                }
                                Ok(r) if matches!(r.stop_reason, acp::StopReason::MaxTokens | acp::StopReason::MaxTurnRequests) => {
                                    t.items.push(Item::Notice { text: "The agent hit its output limit.".into(), error: true })
                                }
                                Ok(r) if matches!(r.stop_reason, acp::StopReason::Refusal) => {
                                    t.items.push(Item::Notice { text: "The agent declined this request.".into(), error: true })
                                }
                                Ok(_) => {}
                                Err(e) => t.items.push(Item::Notice { text: e.to_string(), error: true }),
                            }
                            if t.usage.used == 0 {
                                t.usage.used = t.estimate_tokens();
                            }
                            // Re-write the turn's "Worked for" marker now that its duration is known.
                            t.persisted = t.persisted.min(worked_ix);
                        });
                        ctx.flush();
                    }
                    Command::Cancel => {}
                    Command::Permission(choice) => {
                        if let Some(tx) = ctx.pending.lock().take() {
                            let _ = tx.send(choice);
                        }
                    }
                    Command::SetMode(m) => spawn_set_mode(&conn, &sid, m, &ctx),
                    Command::SetModel(m) => spawn_set_model(&conn, &sid, m, &ctx),
                    Command::Shutdown => break,
                }
            }
            ctx.flush();
            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn spawn_set_mode(conn: &ConnectionTo<Agent>, sid: &acp::SessionId, mode: String, ctx: &Ctx) {
    let (c2, sid, ctx) = (conn.clone(), sid.clone(), ctx.clone());
    let _ = conn.spawn(async move {
        if c2
            .send_request(acp::SetSessionModeRequest::new(sid, mode.clone()))
            .block_task()
            .await
            .is_ok()
        {
            ctx.update(|t| t.mode = Some(mode));
        }
        Ok(())
    });
}

fn spawn_set_model(conn: &ConnectionTo<Agent>, sid: &acp::SessionId, model: String, ctx: &Ctx) {
    let Some(config_id) = ctx.transcript.lock().model_config_id.clone() else {
        return;
    };
    let (c2, sid, ctx) = (conn.clone(), sid.clone(), ctx.clone());
    let _ = conn.spawn(async move {
        let value = acp::SessionConfigOptionValue::from(model.as_str());
        if let Ok(r) = c2
            .send_request(acp::SetSessionConfigOptionRequest::new(
                sid, config_id, value,
            ))
            .block_task()
            .await
        {
            ctx.update(|t| {
                apply_config(t, &r.config_options);
                t.model = Some(model);
            });
        }
        Ok(())
    });
}

#[cfg(test)]
mod tests {
    use super::is_insy_only;
    #[test]
    fn insy_commands() {
        assert!(is_insy_only("insy coord list; insy agent read 4"));
        assert!(is_insy_only(
            "insy agent send 3 \"done\" && insy coord set notes.status done"
        ));
        assert!(!is_insy_only("insy coord list | sh"));
        assert!(!is_insy_only("insy status; rm -rf /"));
        assert!(!is_insy_only("insy coord set x $(cat ~/.ssh/id_rsa)"));
        assert!(!is_insy_only("insy status > out.txt"));
        assert!(!is_insy_only("insy status & curl evil"));
        assert!(!is_insy_only("insyfoo"));
    }
}
