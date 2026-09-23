//! Agent chat tab: transcript, inline permission prompts, composer.
//!
//! The transcript lives in the ACP session's shared state; this view renders
//! it through a virtualized `list` (only visible items are laid out) and only
//! re-lays-out the last item while it streams.

use crate::brain_view::BrainHandle;
use crate::ui::{self, fmt_tokens, icon, monogram};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    ListAlignment, ListState, SharedString, Subscription, Window, div, list, px,
};
use gpui_component::input::{InputEvent, Textarea, TextareaState};
use gpui_component::text::TextView;
use insyde_core::agents::acp::{AcpSession, Block, Policy};
use insyde_core::agents::transcript::{Item, ToolStatus, Transcript};
use insyde_core::agents::{AgentId, AgentSpec};
use insyde_core::store::Store;
use insyde_theme::{ActiveTheme, Theme, metrics};
use std::path::PathBuf;
use std::sync::Arc;

pub enum ChatEvent {
    /// Running state, usage, or permission changed (tab/sidebar indicators).
    Status,
    OpenDiff,
    Handoff,
    CreateBrain,
}

/// Context carried over from another session by "Hand off".
#[derive(Clone)]
pub struct Carried {
    pub from: String,
    pub items: Vec<(String, String)>,
    pub tokens: String,
    pub context: String,
    pub picking_up: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Menu {
    None,
    Model,
    Policy,
}

pub struct ChatView {
    pub agent: AgentId,
    pub title: SharedString,
    pub worktree: PathBuf,
    pub branch: String,
    pub project: String,
    /// Always present; filled from the store for restored sessions.
    transcript: Arc<parking_lot::Mutex<Transcript>>,
    /// The agent process, started lazily on the first message.
    session: Option<AcpSession>,
    session_row: Option<i64>,
    acp_id: Option<String>,
    notify: insyde_core::agents::acp::Notify,
    store: Store,
    composer: Entity<TextareaState>,
    list: ListState,
    count: usize,
    version: u64,
    pub use_brain: bool,
    brain: Option<Arc<BrainHandle>>,
    pub carried: Option<Carried>,
    sent_first: bool,
    pub policy: Policy,
    menu: Menu,
    /// Finished a turn while the tab was not visible.
    pub unseen: bool,
    pub others: String,
    last_running: bool,
    /// A prompt was sent and its turn hasn't finished yet (covers agent start-up).
    awaiting: bool,
    focus: FocusHandle,
    _subs: Vec<Subscription>,
}

impl EventEmitter<ChatEvent> for ChatView {}

impl Focusable for ChatView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

pub struct ChatInit {
    pub agent: AgentId,
    pub title: String,
    pub worktree: PathBuf,
    pub branch: String,
    pub project: String,
    pub store: Store,
    pub brain: Option<Arc<BrainHandle>>,
    pub carried: Option<Carried>,
    /// Resume a stored session.
    pub resume: Option<insyde_core::store::SessionRow>,
    pub policy: Policy,
}

impl ChatView {
    pub fn new(init: ChatInit, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let spec = AgentSpec::get(init.agent);
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 10)
                .submit_on_enter(true)
                .placeholder("Ask for changes, send follow-ups, or paste an error…")
        });
        let mut subs = vec![];
        subs.push(
            cx.subscribe_in(&composer, window, |this, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = ev {
                    this.send(window, cx);
                }
            }),
        );
        let list = ListState::new(0, ListAlignment::Bottom, px(600.));
        list.set_follow_mode(gpui::FollowMode::Tail);

        let (tx, rx) = flume::bounded::<()>(1);
        let notify: insyde_core::agents::acp::Notify = Arc::new(move || {
            let _ = tx.try_send(());
        });
        let session_row = match &init.resume {
            Some(r) => Some(r.id),
            None if spec.acp.is_some() => init
                .store
                .create_session(&init.worktree, spec.key, &init.title)
                .ok(),
            None => None,
        };
        let history: Vec<Item> = init
            .resume
            .as_ref()
            .map(|r| init.store.events(r.id))
            .unwrap_or_default();
        let acp_id = init.resume.as_ref().and_then(|r| r.acp_id.clone());
        let mut restored = Transcript::from_history(history);
        if let Some(r) = &init.resume {
            restored.usage.used = r.tokens.max(0) as u64;
            restored.usage.cost = r.cost;
        }
        let transcript = Arc::new(parking_lot::Mutex::new(restored));
        cx.spawn(async move |this, cx| {
            while rx.recv_async().await.is_ok() {
                if this.update(cx, |this, cx| this.sync(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        let resumed = init.resume.is_some();
        let _ = spec;
        let mut this = Self {
            agent: init.agent,
            title: init.title.into(),
            worktree: init.worktree,
            branch: init.branch,
            project: init.project,
            transcript,
            session: None,
            session_row,
            acp_id,
            notify,
            store: init.store,
            composer,
            list,
            count: 0,
            version: u64::MAX,
            use_brain: true,
            brain: init.brain,
            carried: init.carried,
            sent_first: resumed,
            policy: init.policy,
            menu: Menu::None,
            unseen: false,
            others: String::new(),
            last_running: false,
            awaiting: false,
            focus: cx.focus_handle(),
            _subs: subs,
        };
        this.sync(cx);
        this
    }

    pub fn session_row(&self) -> Option<i64> {
        self.session_row
    }

    pub fn set_brain(&mut self, brain: Option<Arc<BrainHandle>>) {
        self.brain = brain;
    }

    pub fn focus_composer(&self, window: &mut Window, cx: &mut App) {
        self.composer.update(cx, |c, cx| c.focus(window, cx));
    }

    /// Pull new state from the session into the list.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let (n, version, running, has_perm, failed) = {
            let t = self.transcript.lock();
            (
                t.items.len(),
                t.version,
                t.running,
                t.permission.is_some(),
                t.error.is_some(),
            )
        };
        // The turn we were waiting for has finished (or the agent failed to start).
        if (self.last_running && !running) || failed {
            self.awaiting = false;
        }
        if version == self.version {
            return;
        }
        self.version = version;
        if n > self.count {
            self.list.splice(self.count..self.count, n - self.count);
        } else if n < self.count {
            self.list.reset(n);
        }
        self.count = n;
        if n > 0 {
            self.list.remeasure_items(n - 1..n);
        }
        if running != self.last_running || has_perm {
            if self.last_running && !running {
                self.unseen = true;
            }
            if running && !self.last_running {
                self.start_ticker(cx);
            }
            self.last_running = running;
        }
        cx.emit(ChatEvent::Status);
        cx.notify();
    }

    /// Repaint once a second while a turn runs (elapsed-time labels), instead
    /// of animating every frame.
    fn start_ticker(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let running = this.update(cx, |this, cx| {
                    cx.notify();
                    this.is_running()
                });
                if !matches!(running, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Start the agent process if it isn't running yet.
    fn ensure_session(&mut self) -> bool {
        if self.session.is_some() {
            return true;
        }
        let Some(cmd) = AgentSpec::get(self.agent).acp else {
            return false;
        };
        let persist = self.session_row.map(|id| (self.store.clone(), id));
        self.session = Some(AcpSession::start(
            cmd,
            self.worktree.clone(),
            self.acp_id.take(),
            self.transcript.clone(),
            persist,
            self.policy,
            self.notify.clone(),
        ));
        true
    }

    pub fn is_running(&self) -> bool {
        self.awaiting || self.transcript.lock().running
    }

    pub fn needs_attention(&self) -> bool {
        self.transcript.lock().permission.is_some() || self.unseen
    }

    pub fn is_fresh(&self) -> bool {
        self.transcript.lock().items.is_empty()
    }

    /// (used, size) tokens of the context window.
    pub fn context_usage(&self) -> (u64, u64) {
        let t = self.transcript.lock();
        let used = if t.usage.used > 0 {
            t.usage.used
        } else {
            t.estimate_tokens()
        };
        (
            used,
            if t.usage.size > 0 {
                t.usage.size
            } else {
                200_000
            },
        )
    }

    /// Everything a hand-off can carry: (summary, open tasks, edited paths, +, −).
    pub fn handoff_material(&self) -> (String, Vec<String>, Vec<String>, u32, u32) {
        let t = self.transcript.lock();
        let (paths, a, r) = t.edit_totals();
        (t.summary_text(24_000), t.open_tasks(), paths, a, r)
    }

    /// The agent's most recent reply (for `insy agent read` / `wait`).
    pub fn last_reply(&self) -> String {
        let t = self.transcript.lock();
        t.items
            .iter()
            .rev()
            .find_map(|i| match i {
                Item::Agent { text } => Some(text.clone()),
                Item::Notice { text, error: true } => Some(format!("[error] {text}")),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Send `text` as if typed into the composer (used for review comments and teams).
    pub fn send_text(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.composer
            .update(cx, |c, cx| c.set_value(text, window, cx));
        self.send(window, cx);
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().to_string();
        let text = text.trim().to_string();
        if text.is_empty() || !self.ensure_session() {
            return;
        }
        let mut blocks = Vec::new();
        if !self.sent_first {
            if let Some(c) = &self.carried {
                blocks.push(Block::Context {
                    uri: "insyde://handoff".into(),
                    text: c.context.clone(),
                });
            }
            if self.use_brain
                && let Some(b) = &self.brain
            {
                let digest = b.digest(&self.project, &text);
                if !digest.is_empty() {
                    blocks.push(Block::Context {
                        uri: format!("insyde://brain/{}", self.project),
                        text: digest,
                    });
                }
            }
            if let Some(row) = self.session_row {
                let title: String = text.chars().take(60).collect();
                self.store
                    .update_session(row, Some(&title), None, None, None);
                self.title = title.into();
            }
            self.sent_first = true;
        }
        tracing::info!(agent = ?self.agent, chars = text.len(), "send: {}", text.chars().take(80).collect::<String>());
        blocks.push(Block::Text(text));
        if let Some(session) = &self.session {
            session.prompt(blocks);
            self.awaiting = true;
        }
        self.composer
            .update(cx, |c, cx| c.set_value("", window, cx));
        self.unseen = false;
        cx.emit(ChatEvent::Status);
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = &self.session {
            s.cancel();
        }
        cx.notify();
    }

    fn answer(&mut self, option: Option<String>, cx: &mut Context<Self>) {
        if let Some(s) = &self.session {
            s.answer(option);
        }
        cx.emit(ChatEvent::Status);
        cx.notify();
    }

    fn render_item(&self, ix: usize, t: &Theme, cx: &App) -> AnyElement {
        let tr = self.transcript.lock();
        let Some(item) = tr.items.get(ix) else {
            return div().into_any_element();
        };
        let running_turn = tr.running && tr.turn_started.is_some();
        let body: AnyElement = match item {
            Item::User { text } => div()
                .flex()
                .justify_end()
                .child(
                    div()
                        .max_w(gpui::relative(0.78))
                        .px(px(14.))
                        .py(px(10.))
                        .rounded(metrics::RADIUS_LG)
                        .bg(t.panel)
                        .border_1()
                        .border_color(t.line)
                        .text_color(t.ink)
                        .child(text.clone()),
                )
                .into_any_element(),
            Item::Worked { secs } => {
                let is_last_turn = !tr.items[ix + 1..]
                    .iter()
                    .any(|i| matches!(i, Item::Worked { .. }));
                let label = if running_turn && is_last_turn {
                    let el = insyde_core::store::now() - tr.turn_started.unwrap_or(0);
                    format!("Working for {} ›", fmt_dur(el))
                } else {
                    format!("Worked for {} ›", fmt_dur(*secs))
                };
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_3)
                    .child(label)
                    .into_any_element()
            }
            Item::Agent { text } => div()
                .text_color(t.ink_2)
                .child(
                    TextView::markdown(
                        SharedString::from(format!("md-{ix}")),
                        SharedString::from(text.clone()),
                    )
                    .selectable(true),
                )
                .into_any_element(),
            Item::Thought { text } => div()
                .text_size(metrics::TEXT_SM)
                .text_color(t.ink_3)
                .italic()
                .child(ui::trunc(format!(
                    "Thinking · {}",
                    text.lines()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("")
                        .trim()
                )))
                .into_any_element(),
            Item::Tools { calls } => {
                let mut card = div()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .p(px(4.))
                    .bg(t.panel)
                    .border_1()
                    .border_color(t.line)
                    .rounded(px(8.))
                    .text_size(metrics::TEXT_SM);
                for c in calls {
                    let meta_color = match c.status {
                        ToolStatus::Failed => t.err,
                        _ => t.ink_3,
                    };
                    let meta = match c.status {
                        ToolStatus::Running | ToolStatus::Pending if c.meta.is_empty() => {
                            "running…".to_string()
                        }
                        _ => c.meta.clone(),
                    };
                    card = card.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .h(px(30.))
                            .px(px(8.))
                            .rounded(px(5.))
                            .child(
                                div()
                                    .flex_none()
                                    .min_w(px(30.))
                                    .h(px(18.))
                                    .px(px(5.))
                                    .rounded(px(3.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(metrics::TEXT_XS)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .bg(t.hover_2)
                                    .text_color(t.ink_3)
                                    .child(c.kind.label()),
                            )
                            .child(
                                ui::trunc(c.arg.clone())
                                    .flex_1()
                                    .font_family(metrics::MONO_FONT)
                                    .text_size(metrics::TEXT_MONO)
                                    .text_color(t.ink_2),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(metrics::TEXT_XS)
                                    .text_color(meta_color)
                                    .child(meta),
                            ),
                    );
                }
                card.into_any_element()
            }
            Item::Plan { entries } => {
                let mut card = div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .p(px(12.))
                    .bg(t.panel)
                    .border_1()
                    .border_color(t.line)
                    .rounded(px(8.))
                    .text_size(metrics::TEXT_SM);
                card = card.child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.ink)
                        .child("Plan"),
                );
                for e in entries {
                    let (mark, color) = if e.done {
                        ("check", t.ok)
                    } else if e.active {
                        ("running", t.warn)
                    } else {
                        ("minus", t.ink_faint)
                    };
                    card = card.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_color(if e.done { t.ink_3 } else { t.ink_2 })
                            .child(icon(mark, 10., color))
                            .child(e.text.clone()),
                    );
                }
                card.into_any_element()
            }
            Item::Notice { text, error } => div()
                .px(px(12.))
                .py(px(7.))
                .rounded(px(8.))
                .text_size(metrics::TEXT_SM)
                .when(*error, |d| {
                    d.bg(t.err_bg)
                        .border_1()
                        .border_color(t.err_border)
                        .text_color(t.err)
                })
                .when(!*error, |d| d.text_color(t.ink_3))
                .child(text.clone())
                .into_any_element(),
        };
        let _ = cx;
        div()
            .w_full()
            .flex()
            .justify_center()
            .pb(px(14.))
            .child(div().w_full().max_w(px(860.)).px(px(28.)).child(body))
            .into_any_element()
    }

    fn render_empty(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let spec = AgentSpec::get(self.agent);
        let mut col = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .p(px(24.));
        col = col.child(monogram(spec, 36., t)).child(
            div()
                .text_size(metrics::TEXT_TITLE)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.ink)
                .child(format!("New {} session", spec.name)),
        );
        if let Some(c) = &self.carried {
            let mut rows = div().flex().flex_col().gap(px(6.)).px(px(12.)).py(px(10.));
            for (label, tok) in &c.items {
                rows = rows.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(metrics::TEXT_SM)
                        .text_color(t.ink_2)
                        .child(
                            div()
                                .size(px(14.))
                                .rounded_full()
                                .bg(t.palette.green)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("check", 8., t.palette.white)),
                        )
                        .child(div().flex_1().child(label.clone()))
                        .child(
                            div()
                                .text_size(metrics::TEXT_XS)
                                .text_color(t.ink_3)
                                .child(tok.clone()),
                        ),
                );
            }
            col = col.child(
                div()
                    .w(px(380.))
                    .my(px(3.))
                    .bg(t.panel)
                    .border_1()
                    .border_color(t.line)
                    .rounded(px(8.))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(12.))
                            .py(px(8.))
                            .bg(t.sel_bg)
                            .text_color(t.sel_text)
                            .text_size(metrics::TEXT_XS)
                            .font_weight(FontWeight::MEDIUM)
                            .child(icon("handoff", 12., t.sel_text))
                            .child(ui::trunc(format!("Context carried from {}", c.from)).flex_1())
                            .child(div().flex_none().child(c.tokens.clone())),
                    )
                    .child(rows)
                    .when(!c.picking_up.is_empty(), |d| {
                        d.child(
                            div()
                                .px(px(12.))
                                .py(px(8.))
                                .border_t_1()
                                .border_color(t.line_soft)
                                .text_size(metrics::TEXT_XS)
                                .text_color(t.ink_3)
                                .child(format!("Picking up: {}", c.picking_up)),
                        )
                    }),
            );
        }
        col = col.child(
            div()
                .max_w(px(340.))
                .text_center()
                .text_size(metrics::TEXT_SM)
                .text_color(t.ink_3)
                .child(format!(
                    "Shares the {} worktree with {}. Edits are coordinated per file.",
                    self.branch,
                    if self.others.is_empty() {
                        "no other agents"
                    } else {
                        &self.others
                    }
                )),
        );
        if let Some(err) = self.transcript.lock().error.clone() {
            col = col.child(
                div()
                    .max_w(px(420.))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .bg(t.err_bg)
                    .border_1()
                    .border_color(t.err_border)
                    .text_color(t.err)
                    .text_size(metrics::TEXT_SM)
                    .child(err),
            );
        } else if self.session.is_some() && !self.transcript.lock().ready {
            col = col.child(ui::pulse(
                "starting",
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_3)
                    .child(ui::dot(t.warn, 7.))
                    .child(format!("Starting {}…", spec.name)),
            ));
        }
        match self.brain.as_ref().map(|b| b.node_count()) {
            Some(n) if n > 0 => {
                let tokens = self.brain.as_ref().map(|b| b.total_tokens()).unwrap_or(0);
                col = col.child(
                    div()
                        .id("use-brain")
                        .mt(px(6.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .w(px(340.))
                        .px(px(12.))
                        .py(px(10.))
                        .bg(t.panel)
                        .border_1()
                        .border_color(t.line)
                        .rounded(px(8.))
                        .cursor_pointer()
                        .hover({
                            let h = t.hover;
                            move |s| s.bg(h)
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.use_brain = !this.use_brain;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex_1()
                                .child(
                                    div()
                                        .text_size(metrics::TEXT_SM)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(t.ink)
                                        .child("Start with Project Brain"),
                                )
                                .child(
                                    div()
                                        .mt(px(1.))
                                        .text_size(metrics::TEXT_XS)
                                        .text_color(t.ink_3)
                                        .child(format!(
                                            "{n} notes from main · {} tokens, compressed to 18k",
                                            fmt_tokens(tokens)
                                        )),
                                ),
                        )
                        .child(ui::switch(self.use_brain, t)),
                );
            }
            _ => {
                col = col.child(
                    div()
                        .id("create-brain")
                        .mt(px(4.))
                        .text_size(metrics::TEXT_SM)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.ink_2)
                        .underline()
                        .cursor_pointer()
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ChatEvent::CreateBrain)))
                        .child("Create a brain so every agent starts with project context"),
                );
            }
        }
        col.into_any_element()
    }
}

fn fmt_dur(secs: i64) -> String {
    if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let spec = AgentSpec::get(self.agent);
        let fresh = self.is_fresh();
        let (used, size) = self.context_usage();
        let pct = (used * 100 / size.max(1)) as u32;
        let (running, permission, status_line, models, model, tokens_turn) = {
            {
                let tr = self.transcript.lock();
                let status = tr.items.iter().rev().find_map(|i| match i {
                    Item::Tools { calls } => calls
                        .iter()
                        .rev()
                        .find(|c| matches!(c.status, ToolStatus::Running | ToolStatus::Pending))
                        .map(|c| format!("{}…", c.title)),
                    _ => None,
                });
                let model_label = tr.model.as_ref().and_then(|m| {
                    tr.models
                        .iter()
                        .find(|(id, _)| id == m)
                        .map(|(_, n)| n.clone())
                });
                (
                    tr.running,
                    tr.permission.clone(),
                    status,
                    tr.models.clone(),
                    model_label,
                    tr.estimate_tokens(),
                )
            }
        };
        let _ = &window;

        let mut main = div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .bg(t.ground)
            .track_focus(&self.focus);
        if fresh {
            main = main.child(self.render_empty(&t, cx));
        } else {
            let view = cx.entity().downgrade();
            let tt = t.clone();
            main = main.child(
                list(self.list.clone(), move |ix, _w, cx| {
                    view.upgrade()
                        .map(|v| v.read(cx).render_item(ix, &tt, cx))
                        .unwrap_or_else(|| div().into_any_element())
                })
                .flex_1()
                .min_h_0()
                .pt(px(20.)),
            );
        }

        // Live status row, permission prompt, and edit summary sit above the composer.
        let mut dock = div()
            .w_full()
            .max_w(px(860.))
            .mx_auto()
            .px(px(20.))
            .pb(px(16.))
            .flex()
            .flex_col();
        if running && permission.is_none() {
            let elapsed = self
                .transcript
                .lock()
                .turn_started
                .map(|s| insyde_core::store::now() - s)
                .unwrap_or(0);
            dock = dock.child(
                div()
                    .px(px(8.))
                    .pb(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_2)
                    .child(ui::pulse("run-dot", ui::dot(t.warn, 7.)))
                    .child(status_line.unwrap_or_else(|| "Thinking…".into()))
                    .child(div().text_color(t.ink_3).child(format!(
                        "{} · {} tokens",
                        fmt_dur(elapsed),
                        fmt_tokens(tokens_turn)
                    ))),
            );
        }
        if let Some(p) = permission {
            let mut opts = div().flex().gap(px(6.));
            for (i, (id, label, allow)) in p.options.iter().enumerate() {
                let id = id.clone();
                let b = if *allow && i == 0 {
                    ui::primary_button(SharedString::from(format!("perm-{i}")), &t)
                        .h(px(26.))
                        .px(px(10.))
                        .text_size(metrics::TEXT_SM)
                        .child(label.clone())
                } else {
                    ui::small_button(SharedString::from(format!("perm-{i}")), label.clone(), &t)
                };
                opts = opts.child(b.on_click(
                    cx.listener(move |this, _, _, cx| this.answer(Some(id.clone()), cx)),
                ));
            }
            dock = dock.child(
                div()
                    .mb(px(8.))
                    .p(px(12.))
                    .bg(t.panel)
                    .border_1()
                    .border_color(t.accent)
                    .rounded(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(ui::dot(t.accent, 6.))
                            .child(
                                div()
                                    .text_size(metrics::TEXT_SM)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(t.ink)
                                    .child(p.title.clone()),
                            ),
                    )
                    .child(ui::mono_text(&t).child(p.detail.clone()))
                    .child(opts),
            );
        }
        if !running && !fresh {
            let (paths, a, r) = self.transcript.lock().edit_totals();
            if !paths.is_empty() {
                dock = dock.child(
                    div()
                        .mb(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .pl(px(12.))
                        .pr(px(8.))
                        .py(px(8.))
                        .rounded(px(8.))
                        .bg(t.panel)
                        .border_1()
                        .border_color(t.line)
                        .text_size(metrics::TEXT_SM)
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.ink)
                                .child(format!(
                                    "{} changed file{}",
                                    paths.len(),
                                    if paths.len() == 1 { "" } else { "s" }
                                )),
                        )
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.ok)
                                .child(format!("+{a}")),
                        )
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.err)
                                .child(format!("−{r}")),
                        )
                        .child(div().flex_1())
                        .child(
                            ui::small_button("open-diff", "Open diff", &t)
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(ChatEvent::OpenDiff))),
                        ),
                );
            }
        }
        if pct >= 85 && !fresh {
            dock = dock.child(
                div()
                    .mb(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .pl(px(12.))
                    .pr(px(8.))
                    .py(px(7.))
                    .rounded(px(8.))
                    .bg(t.err_bg)
                    .border_1()
                    .border_color(t.err_border)
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.err)
                    .child(ui::dot(t.err, 6.))
                    .child(
                        div()
                            .flex_1()
                            .child(div().flex().gap(px(4.)).child(div().font_weight(FontWeight::SEMIBOLD).child(format!("Context {pct}% full.")))
                            .child("The agent will start compacting soon. Carry this session to a fresh agent to keep full detail.")),
                    )
                    .child(ui::small_button("warn-handoff", "Hand off", &t).on_click(cx.listener(|_, _, _, cx| cx.emit(ChatEvent::Handoff)))),
            );
        }

        // Composer.
        let brain_ready = self.brain.as_ref().is_some_and(|b| b.node_count() > 0);
        let model_label = model.unwrap_or_else(|| spec.name.to_string());
        let send_btn = if running {
            ui::primary_button("stop", &t)
                .size(px(28.))
                .p_0()
                .justify_center()
                .child(icon("stop", 13., t.on_primary))
                .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
        } else {
            ui::primary_button("send", &t)
                .size(px(28.))
                .p_0()
                .justify_center()
                .child(icon("send", 13., t.on_primary))
                .on_click(cx.listener(|this, _, w, cx| this.send(w, cx)))
        };
        let ghost = |id: &'static str, label: String, t: &Theme| {
            let h = t.hover;
            div()
                .id(id)
                .h(px(26.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .cursor_pointer()
                .text_size(metrics::TEXT_SM)
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.ink_2)
                .hover(move |s| s.bg(h))
                .child(label)
        };
        let mut toolbar = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .pt(px(6.))
            .pb(px(8.))
            .child(
                ghost("model", format!("{model_label} ⌄"), &t).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.menu = if this.menu == Menu::Model {
                            Menu::None
                        } else {
                            Menu::Model
                        };
                        cx.notify();
                    },
                )),
            )
            .child(
                ghost("policy", format!("{} ⌄", self.policy.label()), &t).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.menu = if this.menu == Menu::Policy {
                            Menu::None
                        } else {
                            Menu::Policy
                        };
                        cx.notify();
                    },
                )),
            )
            .child(
                div()
                    .px(px(7.))
                    .py(px(2.))
                    .rounded(px(4.))
                    .bg(t.hover_2)
                    .text_size(metrics::TEXT_XS)
                    .text_color(t.ink_3)
                    .child(self.branch.clone()),
            );
        if brain_ready {
            let on = self.use_brain;
            toolbar = toolbar.child(
                div()
                    .id("brain-chip")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(22.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_size(metrics::TEXT_XS)
                    .font_weight(FontWeight::MEDIUM)
                    .bg(if on { t.sel_bg } else { t.hover_2 })
                    .text_color(if on { t.sel_text } else { t.ink_3 })
                    .child(ui::dot(if on { t.accent } else { t.ink_disabled }, 6.))
                    .child("Brain context")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.use_brain = !this.use_brain;
                        cx.notify();
                    })),
            );
        }
        toolbar = toolbar.child(div().flex_1()).child(send_btn);

        let mut composer = div()
            .relative()
            .bg(t.panel)
            .border_1()
            .border_color(t.field_border)
            .rounded(metrics::RADIUS_LG)
            .shadow(t.pop_shadow(false))
            .child(
                div()
                    .px(px(6.))
                    .pt(px(6.))
                    .child(Textarea::new(&self.composer).appearance(false)),
            )
            .child(toolbar);
        if self.menu != Menu::None {
            let mut menu = div()
                .absolute()
                .bottom(px(40.))
                .left(px(8.))
                .w(px(240.))
                .max_h(px(320.))
                .overflow_y_hidden()
                .p(px(4.))
                .bg(t.panel)
                .border_1()
                .border_color(t.line)
                .rounded(metrics::RADIUS_LG)
                .shadow(t.pop_shadow(true))
                .occlude();
            match self.menu {
                Menu::Model => {
                    if models.is_empty() {
                        menu = menu.child(
                            div()
                                .p(px(8.))
                                .text_size(metrics::TEXT_SM)
                                .text_color(t.ink_3)
                                .child("This agent doesn't expose model choices."),
                        );
                    }
                    for (id, name) in models {
                        let h = t.hover;
                        menu = menu.child(
                            div()
                                .id(SharedString::from(format!("m-{id}")))
                                .h(px(30.))
                                .px(px(8.))
                                .flex()
                                .items_center()
                                .rounded(px(6.))
                                .cursor_pointer()
                                .text_size(metrics::TEXT_SM)
                                .text_color(t.ink)
                                .hover(move |s| s.bg(h))
                                .child(name)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(s) = &this.session {
                                        s.set_model(id.clone());
                                    }
                                    this.menu = Menu::None;
                                    cx.notify();
                                })),
                        );
                    }
                }
                Menu::Policy => {
                    for p in [Policy::Ask, Policy::AcceptEdits, Policy::FullAccess] {
                        let h = t.hover;
                        let active = p == self.policy;
                        menu = menu.left(px(120.)).child(
                            div()
                                .id(SharedString::from(format!("p-{}", p.label())))
                                .h(px(30.))
                                .px(px(8.))
                                .flex()
                                .items_center()
                                .justify_between()
                                .rounded(px(6.))
                                .cursor_pointer()
                                .text_size(metrics::TEXT_SM)
                                .text_color(t.ink)
                                .when(active, |d| d.bg(t.hover_2))
                                .hover(move |s| s.bg(h))
                                .child(p.label())
                                .when(active, |d| d.child(icon("check", 10., t.accent)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.policy = p;
                                    if let Some(s) = &this.session {
                                        *s.policy.lock() = p;
                                    }
                                    this.menu = Menu::None;
                                    cx.notify();
                                })),
                        );
                    }
                }
                Menu::None => {}
            }
            composer = composer.child(menu);
        }
        dock = dock.child(composer);
        main.child(dock)
    }
}
