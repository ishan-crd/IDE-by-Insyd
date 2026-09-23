//! Agent teams: start a lead + specialists (each specialist in its own
//! worktree), and show their state. Coordination itself runs through `insy`.

use super::{BottomTab, Workspace};
use crate::ui::{self, monogram};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, FontWeight, SharedString, Window, div, px};
use gpui_component::input::{Textarea, TextareaState};
use insyde_core::agents::{AGENTS, AgentSpec};
use insyde_core::project::{Worktree, WtStatus};
use insyde_core::rpc::Call;
use insyde_core::team::{Started, TeamSpec, lead_prompt, member_prompt};
use insyde_theme::{Theme, metrics};
use serde_json::json;
use std::path::PathBuf;

/// Specialist presets offered by the "Team…" form: (agent key, role).
pub const PRESETS: [(&str, &str); 3] = [
    ("codex", "implementer"),
    ("claude", "tests"),
    ("claude", "review"),
];

pub struct TeamState {
    pub task: String,
    pub lead: Started,
    pub members: Vec<Started>,
}

pub struct TeamForm {
    pub task: Entity<TextareaState>,
    pub picks: [bool; 3],
}

impl Workspace {
    pub fn open_team_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_open = false;
        let task = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 8)
                .placeholder("What should the team do?")
        });
        task.update(cx, |s, cx| s.focus(window, cx));
        self.team_form = Some(TeamForm {
            task,
            picks: [true, true, false],
        });
        cx.notify();
    }

    fn submit_team_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.team_form.take() else {
            return;
        };
        let task = form.task.read(cx).value().to_string();
        if task.trim().is_empty() {
            self.toast("Describe the task for the team", true, cx);
            self.team_form = Some(form);
            return;
        }
        let picks: Vec<(&str, &str)> = PRESETS
            .iter()
            .zip(form.picks)
            .filter(|(_, on)| *on)
            .map(|(p, _)| *p)
            .collect();
        if picks.is_empty() {
            self.toast("Pick at least one specialist", true, cx);
            self.team_form = Some(form);
            return;
        }
        let spec = TeamSpec::default_for(&task, &picks);
        self.start_team(spec, None, window, cx);
    }

    pub(super) fn rpc_team_run_impl(
        &mut self,
        call: Call,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let spec = match call.param_str("spec").map(|s| TeamSpec::parse(&s)) {
            Some(Ok(s)) => s,
            Some(Err(e)) => return call.err(format!("bad team spec: {e}")),
            None => return call.err("need spec"),
        };
        self.start_team(spec, Some(call), window, cx);
    }

    /// Create specialist worktrees (off the UI thread), then open and brief every member.
    pub fn start_team(
        &mut self,
        spec: TeamSpec,
        reply: Option<Call>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fail = |reply: Option<Call>, this: &mut Self, cx: &mut Context<Self>, msg: String| {
            this.toast(msg.clone(), true, cx);
            if let Some(r) = reply {
                r.err(msg);
            }
        };
        let (Some(ps), Some(lead_path)) = (self.project(), self.active_wt_path()) else {
            return fail(reply, self, cx, "Open a project first".into());
        };
        for m in std::iter::once(&spec.lead).chain(&spec.specialists) {
            match AgentSpec::by_key(&m.agent) {
                Some(s) if s.acp.is_some() => {}
                _ => {
                    return fail(
                        reply,
                        self,
                        cx,
                        format!("Agent `{}` can't join a team (needs chat mode)", m.agent),
                    );
                }
            }
        }
        let (root, base, pi) = (ps.project.root.clone(), ps.project.base.clone(), self.p);
        let jobs: Vec<(usize, Option<String>)> = spec
            .specialists
            .iter()
            .enumerate()
            .map(|(i, m)| (i, m.worktree.then(|| spec.branch_for(&m.role))))
            .collect();
        self.toast(
            format!("Starting a team of {}…", spec.specialists.len() + 1),
            false,
            cx,
        );
        let lp = lead_path.clone();
        let task = cx.background_spawn(async move {
            let mut out = Vec::new();
            for (i, branch) in jobs {
                match branch {
                    Some(b) => match insyde_core::git::add_worktree(&root, &b, &base) {
                        Ok(p) => out.push((i, p, b)),
                        Err(e) => return Err(format!("worktree {b}: {e}")),
                    },
                    None => out.push((i, lp.clone(), String::new())),
                }
            }
            Ok(out)
        });
        cx.spawn_in(window, async move |this, cx| {
            let created = task.await;
            let _ = this.update_in(cx, |this, window, cx| match created {
                Err(e) => fail(reply, this, cx, e),
                Ok(created) => this.brief_team(spec, pi, lead_path, created, reply, window, cx),
            });
        })
        .detach();
    }

    #[allow(clippy::too_many_arguments)]
    fn brief_team(
        &mut self,
        spec: TeamSpec,
        pi: usize,
        lead_path: PathBuf,
        created: Vec<(usize, PathBuf, String)>,
        reply: Option<Call>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Make new worktrees known immediately (the periodic scan fills in stats).
        let lead_branch = self.active_branch();
        if let Some(ps) = self.projects.get_mut(pi) {
            for (_, path, branch) in &created {
                if !ps.project.worktrees.iter().any(|w| &w.path == path) {
                    ps.project.worktrees.push(Worktree {
                        path: path.clone(),
                        branch: branch.clone(),
                        head: String::new(),
                        primary: false,
                        stat: Default::default(),
                        last_commit: None,
                        pr: None,
                        status: WtStatus::Plain,
                    });
                }
            }
        }
        let open = |this: &mut Self,
                    path: &PathBuf,
                    agent: &str,
                    window: &mut Window,
                    cx: &mut Context<Self>|
         -> Option<u64> {
            this.focus_worktree(path, window, cx);
            let id = AgentSpec::by_key(agent)?.id;
            this.open_chat(id, None, None, window, cx);
            this.wt().and_then(|w| w.tabs.last()).map(|t| t.id)
        };
        self.suppress_default_tab = true;
        let lead_id = open(self, &lead_path, &spec.lead.agent, window, cx);
        let Some(lead_id) = lead_id else {
            self.suppress_default_tab = false;
            if let Some(r) = reply {
                r.err("couldn't open the lead");
            }
            return;
        };
        let mut members = Vec::new();
        for (i, path, branch) in &created {
            let m = &spec.specialists[*i];
            let branch = if branch.is_empty() {
                lead_branch.clone()
            } else {
                branch.clone()
            };
            if let Some(id) = open(self, path, &m.agent, window, cx) {
                if let Some(chat) = self.chat_by_id(id) {
                    let prompt = member_prompt(&spec, m, lead_id, &branch);
                    let title = format!("{} · team", m.role);
                    chat.update(cx, |c, cx| {
                        c.send_text(prompt, window, cx);
                        c.set_title(title);
                    });
                }
                members.push(Started {
                    role: m.role.clone(),
                    agent: m.agent.clone(),
                    id,
                    worktree: path.display().to_string(),
                    branch,
                });
            }
        }
        self.suppress_default_tab = false;
        self.focus_worktree(&lead_path, window, cx);
        if let Some(ws) = self.wt_mut()
            && let Some(i) = ws.tabs.iter().position(|t| t.id == lead_id)
        {
            ws.active = i;
        }
        if let Some(chat) = self.chat_by_id(lead_id) {
            let prompt = lead_prompt(&spec, &members);
            chat.update(cx, |c, cx| {
                c.send_text(prompt, window, cx);
                c.set_title("lead · team".into());
            });
        }
        let lead = Started {
            role: "lead".into(),
            agent: spec.lead.agent.clone(),
            id: lead_id,
            worktree: lead_path.display().to_string(),
            branch: lead_branch,
        };
        self.store.set(&self.coord_key("team.task"), &spec.task);
        for m in &members {
            self.store.set(
                &self.coord_key(&format!("{}.status", m.role)),
                &"working".to_string(),
            );
        }
        let ids: Vec<u64> = members.iter().map(|m| m.id).collect();
        if let Some(r) = reply {
            r.ok(json!({ "lead": lead_id, "members": ids, "message": format!("team started: lead {lead_id}, members {ids:?}") }));
        }
        self.team = Some(TeamState {
            task: spec.task.clone(),
            lead,
            members,
        });
        self.bottom_tab = BottomTab::Team;
        self.scan_project(pi, cx);
        cx.notify();
    }

    pub(super) fn render_team_form(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(form) = &self.team_form else {
            return div().into_any_element();
        };
        let mut picks = div().flex().flex_col().gap(px(2.));
        for (i, (agent, role)) in PRESETS.iter().enumerate() {
            let on = form.picks[i];
            let spec = AgentSpec::by_key(agent).unwrap_or(&AGENTS[2]);
            let hover = t.hover;
            picks = picks.child(
                div()
                    .id(SharedString::from(format!("tp-{i}")))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(34.))
                    .px(px(6.))
                    .rounded(metrics::RADIUS)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(ui::checkbox(on, t))
                    .child(monogram(spec, 18., t))
                    .child(
                        div()
                            .flex_1()
                            .text_size(metrics::TEXT_SM)
                            .child(format!("{} · {}", spec.name, role)),
                    )
                    .child(
                        div()
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child("own worktree"),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(f) = &mut this.team_form {
                            f.picks[i] = !f.picks[i];
                        }
                        cx.notify();
                    })),
            );
        }
        div()
            .absolute()
            .top(px(120.))
            .left_1_2()
            .ml(px(-210.))
            .w(px(420.))
            .bg(t.panel)
            .border_1()
            .border_color(t.line)
            .rounded(metrics::RADIUS_LG)
            .shadow(t.pop_shadow(true))
            .occlude()
            .child(
                div()
                    .px(px(14.))
                    .pt(px(12.))
                    .pb(px(10.))
                    .border_b_1()
                    .border_color(t.line)
                    .child(div().text_size(metrics::TEXT).font_weight(FontWeight::SEMIBOLD).child("Start a team"))
                    .child(div().mt(px(2.)).text_size(metrics::TEXT_XS).text_color(t.ink_3).child(format!(
                        "Claude Code leads in {}; each specialist gets its own worktree and reports back through insy.",
                        self.active_branch()
                    ))),
            )
            .child(div().p(px(10.)).child(div().bg(t.panel_2).border_1().border_color(t.field_border).rounded(px(8.)).p(px(6.)).child(Textarea::new(&form.task).appearance(false))))
            .child(div().px(px(10.)).pb(px(8.)).child(div().px(px(4.)).pb(px(4.)).text_size(metrics::TEXT_XS).text_color(t.ink_3).child("Specialists")).child(picks))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(t.line)
                    .bg(t.panel_2)
                    .child(div().flex_1().text_size(metrics::TEXT_XS).text_color(t.ink_3).child("Or run `insy team run team.toml`"))
                    .child(ui::small_button("team-cancel", "Cancel", t).h(px(28.)).on_click(cx.listener(|this, _, _, cx| {
                        this.team_form = None;
                        cx.notify();
                    })))
                    .child(
                        ui::primary_button("team-go", t)
                            .h(px(28.))
                            .px(px(12.))
                            .text_size(metrics::TEXT_SM)
                            .child("Start team")
                            .on_click(cx.listener(|this, _, w, cx| this.submit_team_form(w, cx))),
                    ),
            )
            .into_any_element()
    }

    /// Bottom-strip Team tab: members, their state, and shared coordination entries.
    pub(super) fn render_team_tab(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(team) = &self.team else {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .bg(t.panel_2)
                .text_size(metrics::TEXT_SM)
                .text_color(t.ink_3)
                .child("No team yet. Pick “Team…” in the agent menu, or run `insy team run team.toml`.")
                .into_any_element();
        };
        let mut rows = div().flex().flex_col().gap(px(2.));
        for m in std::iter::once(&team.lead).chain(&team.members) {
            let running = self
                .chat_by_id(m.id)
                .is_some_and(|c| c.read(cx).is_running());
            let spec = AgentSpec::by_key(&m.agent).unwrap_or(&AGENTS[2]);
            let status: Option<String> = self
                .store
                .get(&self.coord_key(&format!("{}.status", m.role)));
            let path = PathBuf::from(&m.worktree);
            let hover = t.hover;
            rows = rows.child(
                div()
                    .id(SharedString::from(format!("tm-{}", m.id)))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(30.))
                    .px(px(8.))
                    .rounded(metrics::RADIUS)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(monogram(spec, 18., t))
                    .child(
                        div()
                            .w(px(90.))
                            .text_size(metrics::TEXT_SM)
                            .font_weight(FontWeight::MEDIUM)
                            .child(m.role.clone()),
                    )
                    .child(
                        div()
                            .w(px(56.))
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child(format!("agent {}", m.id)),
                    )
                    .child(
                        ui::trunc(m.branch.clone())
                            .flex_1()
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_2),
                    )
                    .child(
                        div()
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child(status.unwrap_or_default()),
                    )
                    .child(if running {
                        ui::pulse(
                            SharedString::from(format!("tmr-{}", m.id)),
                            ui::dot(t.ok, 7.),
                        )
                    } else {
                        ui::dot(t.ink_disabled, 7.).into_any_element()
                    })
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.focus_worktree(&path, w, cx);
                    })),
            );
        }
        let prefix = self.coord_key("");
        let mut coord = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .font_family(metrics::MONO_FONT)
            .text_size(metrics::TEXT_MONO);
        for (k, v) in self.store.kv_prefix(&prefix) {
            let v: String = serde_json::from_str(&v).unwrap_or(v);
            coord = coord.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_none()
                            .text_color(t.ink_3)
                            .child(k[prefix.len()..].to_string()),
                    )
                    .child(ui::trunc(v).text_color(t.ink_2)),
            );
        }
        div()
            .id("team-tab")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .bg(t.panel_2)
            .p(px(10.))
            .flex()
            .gap(px(16.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        ui::trunc(format!("Task: {}", team.task))
                            .text_size(metrics::TEXT_SM)
                            .text_color(t.ink),
                    )
                    .child(rows),
            )
            .child(
                div()
                    .w(px(320.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child("Shared state (insy coord)"),
                    )
                    .child(coord),
            )
            .into_any_element()
    }
}
