//! Handlers for the local control API (see `insyde_core::rpc` and the `insy` CLI).
//! Every call runs on the UI thread with full access to workspace state.

use super::{TabView, Workspace};
use gpui::{AppContext, Context, Window};
use insyde_core::agents::{AGENTS, AgentSpec};
use insyde_core::rpc::Call;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

impl Workspace {
    /// Start serving the socket and dispatching calls to this workspace.
    pub(super) fn start_rpc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (tx, rx) = flume::unbounded::<Call>();
        let path = insyde_core::rpc::socket_path();
        match insyde_core::rpc::serve(&path, tx) {
            Ok(()) => self.log(format!("Control socket at {}", path.display())),
            Err(e) => {
                self.log(format!(
                    "Control socket unavailable ({e}); is another InsyDE running?"
                ));
                return;
            }
        }
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(call) = rx.recv_async().await {
                if this
                    .update_in(cx, |this, window, cx| this.handle_call(call, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn handle_call(&mut self, call: Call, window: &mut Window, cx: &mut Context<Self>) {
        let method = call.request.method.clone();
        match method.as_str() {
            "status" => {
                let (project, worktree, branch) = (
                    self.project().map(|p| p.project.name.clone()),
                    self.active_wt_path(),
                    self.active_branch(),
                );
                call.ok(json!({ "project": project, "worktree": worktree, "branch": branch, "agents": self.agents_json(cx) }));
            }
            "worktree.list" => {
                let active = self.active_wt_path();
                let list: Vec<Value> = self
                    .project()
                    .map(|p| {
                        p.project
                            .worktrees
                            .iter()
                            .map(|w| json!({ "branch": w.branch, "path": w.path, "primary": w.primary, "active": Some(&w.path) == active.as_ref() }))
                            .collect()
                    })
                    .unwrap_or_default();
                call.ok(json!({ "worktrees": list }));
            }
            "worktree.create" => match call.param_str("title") {
                Some(t) if !t.trim().is_empty() => self.create_worktree(t, Some(call), cx),
                _ => call.err("missing title"),
            },
            "agent.list" => call.ok(json!({ "agents": self.agents_json(cx) })),
            "agent.new" => self.rpc_agent_new(call, window, cx),
            "agent.send" => {
                let (Some(id), Some(text)) = (call.param_u64("id"), call.param_str("text")) else {
                    return call.err("need id and text");
                };
                match self.chat_by_id(id) {
                    Some(chat) => {
                        chat.update(cx, |c, cx| c.send_text(text, window, cx));
                        call.ok(json!({ "message": format!("sent to agent {id}") }));
                    }
                    None => call.err(format!(
                        "no chat agent with id {id} (see `insy agent list`)"
                    )),
                }
            }
            "agent.get" => {
                let Some(id) = call.param_u64("id") else {
                    return call.err("need id");
                };
                match self.chat_by_id(id) {
                    Some(chat) => {
                        let c = chat.read(cx);
                        let (used, size) = c.context_usage();
                        call.ok(json!({ "id": id, "running": c.is_running(), "last_reply": c.last_reply(), "context_used": used, "context_size": size }));
                    }
                    None => call.err(format!("no chat agent with id {id}")),
                }
            }
            "brain.search" => {
                let q = call.param_str("query").unwrap_or_default();
                match self.brain_handle(self.p) {
                    Some(b) => {
                        let ids = b.brain.search(&q, 20);
                        let g = b.graph.read();
                        let results: Vec<Value> = ids
                            .into_iter()
                            .filter_map(|i| g.nodes.get(i))
                            .map(|n| json!({ "kind": n.kind.label(), "name": n.name, "path": n.path, "summary": n.summary }))
                            .collect();
                        call.ok(json!({ "results": results }));
                    }
                    None => call.err("no Project Brain yet: create one from the top bar"),
                }
            }
            "open" => {
                let Some(path) = call.param_str("path") else {
                    return call.err("need path");
                };
                let path = insyde_core::remote::parse_target(&path)
                    .unwrap_or_else(|| PathBuf::from(&path));
                self.open_project(path, Some(call), window, cx);
            }
            "coord.get" => {
                let key = call.param_str("key").unwrap_or_default();
                let v: Option<String> = self.store.get(&self.coord_key(&key));
                call.ok(json!({ "value": v }));
            }
            "coord.set" => {
                let (Some(key), Some(value)) = (call.param_str("key"), call.param_str("value"))
                else {
                    return call.err("need key and value");
                };
                self.store.set(&self.coord_key(&key), &value);
                self.log(format!("coord {key} = {value}"));
                cx.notify();
                call.ok(json!({ "message": "ok" }));
            }
            "coord.list" => {
                let prefix = self.coord_key("");
                let entries: serde_json::Map<String, Value> = self
                    .store
                    .kv_prefix(&prefix)
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            k[prefix.len()..].to_string(),
                            serde_json::from_str(&v).unwrap_or(Value::String(v)),
                        )
                    })
                    .collect();
                call.ok(json!({ "entries": entries }));
            }
            "team.run" => self.rpc_team_run(call, window, cx),
            other => call.err(format!("unknown method {other}")),
        }
    }

    /// Coordination state is scoped to the active project.
    pub(crate) fn coord_key(&self, key: &str) -> String {
        let root = self
            .project()
            .map(|p| p.project.root.display().to_string())
            .unwrap_or_default();
        format!("coord:{root}:{key}")
    }

    fn agents_json(&self, cx: &gpui::App) -> Vec<Value> {
        let mut out = vec![];
        for ps in &self.projects {
            for w in &ps.project.worktrees {
                let Some(ws) = self.wts.get(&w.path) else {
                    continue;
                };
                for t in ws.tabs.iter().filter(|t| t.is_agent()) {
                    let kind = if matches!(t.view, TabView::Chat(_)) {
                        "chat"
                    } else {
                        "terminal"
                    };
                    out.push(json!({
                        "id": t.id,
                        "agent": AgentSpec::get(t.agent).key,
                        "kind": kind,
                        "title": t.title(cx).to_string(),
                        "running": t.running(cx),
                        "branch": w.branch,
                        "worktree": w.path,
                    }));
                }
            }
        }
        out
    }

    pub(crate) fn chat_by_id(&self, id: u64) -> Option<gpui::Entity<crate::chat::ChatView>> {
        self.wts
            .values()
            .flat_map(|ws| ws.tabs.iter())
            .find(|t| t.id == id)
            .and_then(|t| match &t.view {
                TabView::Chat(c) => Some(c.clone()),
                TabView::Term(_) | TabView::File(_) => None,
            })
    }

    /// Switch the UI to the worktree at `path` (any open project).
    pub(crate) fn focus_worktree(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        for (pi, ps) in self.projects.iter().enumerate() {
            if let Some(wi) = ps.project.worktrees.iter().position(|w| w.path == path) {
                if pi != self.p {
                    self.select_project(pi, window, cx);
                }
                self.select_wt(wi, window, cx);
                return true;
            }
        }
        false
    }

    fn rpc_agent_new(&mut self, call: Call, window: &mut Window, cx: &mut Context<Self>) {
        let name = call
            .param_str("agent")
            .unwrap_or_else(|| "claude".into())
            .to_lowercase();
        let Some(spec) = AGENTS.iter().find(|a| {
            a.key == name
                || a.name.to_lowercase() == name
                || a.name.to_lowercase().starts_with(&name)
        }) else {
            return call.err(format!("unknown agent {name}"));
        };
        if spec.acp.is_none() {
            return call.err(format!(
                "{} has no chat mode; open it from the agent picker as a terminal",
                spec.name
            ));
        }
        if let Some(wt) = call.param_str("worktree") {
            let wt = PathBuf::from(wt);
            if Some(&wt) != self.active_wt_path().as_ref() && !self.focus_worktree(&wt, window, cx)
            {
                return call.err(format!(
                    "{} is not a known worktree (wait for `insy worktree create` to finish)",
                    wt.display()
                ));
            }
        }
        self.open_chat(spec.id, None, None, window, cx);
        let Some(id) = self.wt().and_then(|w| w.tabs.last()).map(|t| t.id) else {
            return call.err("couldn't open the agent");
        };
        if let (Some(prompt), Some(chat)) = (call.param_str("prompt"), self.chat_by_id(id)) {
            chat.update(cx, |c, cx| c.send_text(prompt, window, cx));
        }
        call.ok(json!({ "id": id, "message": format!("started {} as agent {id}", spec.name) }));
    }

    /// Add a repository by path (or switch to it if already open).
    /// Add a repository (local path or `ssh://host/path`) or switch to it if
    /// already open. The git probe runs off the UI thread (it may be remote).
    pub(crate) fn open_project(
        &mut self,
        path: PathBuf,
        reply: Option<Call>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let remote = insyde_core::remote::is_remote(&path);
        if remote {
            self.toast(format!("Connecting to {}…", path.display()), false, cx);
        }
        let task = cx.background_spawn(async move { insyde_core::project::Project::probe(&path) });
        cx.spawn_in(window, async move |this, cx| {
            let probe = task.await;
            let _ = this.update_in(cx, |this, window, cx| match probe {
                Ok(row) => {
                    if let Some(i) = this
                        .projects
                        .iter()
                        .position(|p| p.project.root == row.path)
                    {
                        this.select_project(i, window, cx);
                    } else {
                        let _ = this.store.add_project(&row);
                        this.projects.push(super::ProjectState {
                            project: insyde_core::project::Project::from_row(&row),
                            active_wt: 0,
                            brain: super::BrainState::None,
                            scanning: false,
                        });
                        let i = this.projects.len() - 1;
                        this.select_project(i, window, cx);
                        this.toast(format!("Opened {}", row.name), false, cx);
                    }
                    if let Some(r) = reply {
                        r.ok(json!({ "message": format!("opened {}", row.name) }));
                    }
                }
                Err(e) => {
                    let msg = if remote {
                        format!("Couldn't open over SSH: {e}")
                    } else {
                        format!("Not a git repository: {e}")
                    };
                    this.toast(msg.clone(), true, cx);
                    if let Some(r) = reply {
                        r.err(msg);
                    }
                }
            });
        })
        .detach();
    }

    fn rpc_team_run(&mut self, call: Call, window: &mut Window, cx: &mut Context<Self>) {
        self.rpc_team_run_impl(call, window, cx);
    }
}
