//! Root view: owns projects, worktrees, agent tabs, terminals and layout.
//! Rendering is split by region of the design (top bar, sidebar, center,
//! right panel, terminals, overlays); state and actions live here.

mod bottom;
mod center;
mod files;
mod overlays;
mod right;
mod rpc;
mod side;
mod team;
mod top;

use crate::brain_view::{BrainEvent, BrainHandle, BrainView};
use crate::chat::{Carried, ChatEvent, ChatInit, ChatView};
use crate::terminal_view::{TerminalEvent, TerminalView};
use gpui::prelude::*;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, KeyDownEvent, MouseButton, MouseMoveEvent,
    ScrollDelta, ScrollWheelEvent, SharedString, Subscription, Window, actions, div, px,
};
use gpui_component::input::{InputEvent, InputState};
use insyde_core::agents::acp::Policy;
use insyde_core::agents::{AgentId, AgentSpec, DEFAULT_AGENT};
use insyde_core::brain::Brain;
use insyde_core::forge::PullRequest;
use insyde_core::git::FileStat;
use insyde_core::project::Project;
use insyde_core::store::Store;
use insyde_theme::{ActiveTheme, metrics};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

actions!(
    insyde,
    [
        Quit,
        NewAgent,
        CloseTab,
        ToggleSidebar,
        ToggleTerminals,
        ToggleRight,
        ToggleTheme,
        NewTerminal,
        OpenProject,
        SaveFile,
        OpenSettings
    ]
);

/// (path, depth, is_dir) rows of the expanded file tree.
pub type TreeRows = Vec<(PathBuf, usize, bool)>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SideTab {
    Worktrees,
    Files,
    Search,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RightTab {
    Checks,
    Diff,
    Editor,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Terminals,
    Logs,
    Problems,
    Team,
}

pub enum TabView {
    Chat(Entity<ChatView>),
    Term(Entity<TerminalView>),
    /// A file opened with "Open in New Tab" from the file tree.
    File(Entity<crate::editor::FileEditor>),
}

pub struct AgentTab {
    pub id: u64,
    pub agent: AgentId,
    pub view: TabView,
}

impl AgentTab {
    pub fn title(&self, cx: &App) -> SharedString {
        match &self.view {
            TabView::Chat(c) => c.read(cx).title.clone(),
            TabView::Term(t) => t.read(cx).title.clone(),
            TabView::File(e) => {
                let rel = &e.read(cx).rel;
                rel.rsplit('/').next().unwrap_or(rel).to_string().into()
            }
        }
    }
    pub fn running(&self, cx: &App) -> bool {
        match &self.view {
            TabView::Chat(c) => c.read(cx).is_running(),
            TabView::Term(t) => t.read(cx).is_busy(),
            TabView::File(_) => false,
        }
    }
    /// Agent tabs (chat or terminal), as opposed to file tabs.
    pub fn is_agent(&self) -> bool {
        !matches!(self.view, TabView::File(_))
    }
}

/// The file tree's right-click menu.
pub struct FileMenu {
    pub path: PathBuf,
    pub rel: String,
    pub is_dir: bool,
    /// Where the menu opens, in window coordinates.
    pub pos: gpui::Point<gpui::Pixels>,
    /// Rename / New File / New Folder: the name being typed.
    pub edit: Option<(NameEdit, Entity<InputState>)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NameEdit {
    Rename,
    NewFile,
    NewFolder,
}

pub struct Pane {
    pub view: Entity<TerminalView>,
    pub frac: f32,
}

/// UI state of one worktree (created on first visit).
pub struct WtState {
    pub tabs: Vec<AgentTab>,
    pub active: usize,
    pub panes: Vec<Pane>,
    pub pr: Option<PullRequest>,
    pub files: Vec<FileStat>,
    pub diff_sel: Option<String>,
    pub patch: Option<Arc<Vec<insyde_core::git::PatchLine>>>,
    /// Review comments not yet sent to an agent.
    pub comments: Vec<insyde_core::store::Comment>,
    /// Comment being written: (file, line, side, code, input).
    pub draft: Option<(String, i64, String, String, Entity<InputState>)>,
    pub editor: Option<Entity<crate::editor::FileEditor>>,
    /// (command, button label) for the top bar's Run button.
    pub run: Option<(String, String)>,
    pub loading_pr: bool,
}

pub enum BrainState {
    None,
    Building {
        pct: u8,
        label: String,
    },
    Ready {
        handle: Arc<BrainHandle>,
        updated: i64,
    },
}

pub struct ProjectState {
    pub project: Project,
    pub active_wt: usize,
    pub brain: BrainState,
    pub scanning: bool,
}

#[derive(Clone, Copy)]
pub enum Drag {
    Side {
        x0: f32,
        w0: f32,
    },
    Right {
        x0: f32,
        w0: f32,
    },
    Term {
        y0: f32,
        h0: f32,
    },
    Pane {
        idx: usize,
        x0: f32,
        fr0: (f32, f32),
        width: f32,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct LayoutPrefs {
    side_w: f32,
    right_w: f32,
    term_h: f32,
    show_side: bool,
    show_right: bool,
    show_term: bool,
}

impl Default for LayoutPrefs {
    fn default() -> Self {
        Self {
            side_w: metrics::SIDE_W,
            right_w: metrics::RIGHT_W,
            term_h: metrics::TERM_H,
            show_side: true,
            show_right: true,
            show_term: true,
        }
    }
}

pub struct Handoff {
    pub target: usize,
    pub opts: [bool; 5],
    /// Estimated tokens per option (summary, diff, terminals, tasks, brain).
    pub tokens: [f64; 5],
    pub subs: [String; 5],
    pub summary: String,
    pub diff: String,
    pub terms: String,
    pub tasks: Vec<String>,
    pub brain: String,
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

/// Gap around the floating sheets in glass mode.
pub(crate) const GLASS_GAP: f32 = 6.;

pub struct Workspace {
    pub store: Store,
    pub projects: Vec<ProjectState>,
    pub p: usize,
    pub wts: HashMap<PathBuf, WtState>,
    pub side_tab: SideTab,
    pub right_tab: RightTab,
    pub bottom_tab: BottomTab,
    prefs: LayoutPrefs,
    drag: Option<Drag>,
    pub menu_open: bool,
    pub handoff: Option<Handoff>,
    pub history_open: bool,
    /// The Run button's menu, and its "any command" input.
    pub run_menu: Option<Entity<InputState>>,
    pub file_menu: Option<FileMenu>,
    pub brain_view: Option<Entity<BrainView>>,
    pub new_wt: Option<Entity<InputState>>,
    pub search: Entity<InputState>,
    pub search_results: Vec<(String, usize, String)>,
    pub tree_open: std::collections::HashSet<PathBuf>,
    /// Visible file-tree rows for (worktree, expanded dirs); rebuilt only on change.
    pub tree_cache: Option<(PathBuf, usize, Arc<TreeRows>)>,
    pub confirm_delete: Option<PathBuf>,
    pub logs: Vec<String>,
    pub toast: Option<Toast>,
    swipe_acc: f32,
    pub swipe_dx: f32,
    swipe_lock: Option<Instant>,
    next_id: u64,
    focus: FocusHandle,
    pub window_size: (f32, f32),
    pending_default_tab: bool,
    pub ahead_behind: HashMap<PathBuf, (u32, u32)>,
    pub team: Option<team::TeamState>,
    pub team_form: Option<team::TeamForm>,
    pub connect_form: Option<Entity<InputState>>,
    pub settings_view: Option<Entity<crate::settings_view::SettingsView>>,
    /// Setup commands to run in the first terminal of freshly created worktrees.
    pending_setup: HashMap<PathBuf, String>,
    /// Set while a team is opening worktrees, so they don't get a default chat.
    suppress_default_tab: bool,
    /// Native window background last applied (follows the glass setting).
    bg_appearance: gpui::WindowBackgroundAppearance,
    _subs: Vec<Subscription>,
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Workspace {
    pub fn new(store: Store, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefs: LayoutPrefs = store.get("layout").unwrap_or_default();
        let mut rows = store.projects().unwrap_or_default();
        // First launch from inside a repository: adopt it.
        if rows.is_empty()
            && let Ok(cwd) = std::env::current_dir()
            && let Ok(row) = Project::probe(&cwd)
        {
            let _ = store.add_project(&row);
            rows.push(row);
        }
        let projects: Vec<ProjectState> = rows
            .iter()
            .map(|r| ProjectState {
                project: Project::from_row(r),
                active_wt: 0,
                brain: BrainState::None,
                scanning: false,
            })
            .collect();
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search files in worktree"));
        let subs = vec![
            cx.subscribe_in(&search, window, |this, s, ev: &InputEvent, _, cx| {
                if let InputEvent::PressEnter { .. } | InputEvent::Change = ev {
                    let q = s.read(cx).value().to_string();
                    this.run_search(q, cx);
                }
            }),
        ];
        let p = store
            .get::<usize>("active_project")
            .unwrap_or(0)
            .min(projects.len().saturating_sub(1));
        let mut this = Self {
            store,
            projects,
            p,
            wts: HashMap::new(),
            side_tab: SideTab::Worktrees,
            right_tab: RightTab::Checks,
            bottom_tab: BottomTab::Terminals,
            prefs,
            drag: None,
            menu_open: false,
            handoff: None,
            history_open: false,
            run_menu: None,
            file_menu: None,
            brain_view: None,
            new_wt: None,
            search,
            search_results: vec![],
            tree_open: Default::default(),
            tree_cache: None,
            confirm_delete: None,
            logs: vec![],
            toast: None,
            swipe_acc: 0.,
            swipe_dx: 0.,
            swipe_lock: None,
            next_id: 1,
            focus: cx.focus_handle(),
            window_size: (1512., 982.),
            pending_default_tab: false,
            ahead_behind: HashMap::new(),
            team: None,
            team_form: None,
            connect_form: None,
            settings_view: None,
            pending_setup: HashMap::new(),
            suppress_default_tab: false,
            bg_appearance: crate::prefs::window_background(),
            _subs: subs,
        };
        for i in 0..this.projects.len() {
            this.scan_project(i, cx);
            this.load_brain(i, cx);
        }
        if insyde_core::settings::get().control_socket {
            this.start_rpc(window, cx);
        }
        // Refresh the active project's worktrees and PR state periodically.
        cx.spawn(async move |this, cx| {
            loop {
                let secs = insyde_core::settings::get().refresh_secs.clamp(5, 600) as u64;
                cx.background_executor()
                    .timer(Duration::from_secs(secs))
                    .await;
                if this
                    .update(cx, |this, cx| this.scan_project(this.p, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        this
    }

    // ---------- helpers ----------

    pub fn log(&mut self, s: impl Into<String>) {
        let s = s.into();
        tracing::info!("{s}");
        self.logs.push(s);
        if self.logs.len() > 500 {
            self.logs.drain(..100);
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        let text = text.into();
        self.log(text.clone());
        self.toast = Some(Toast {
            text,
            error,
            at: Instant::now(),
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(4200))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .toast
                    .as_ref()
                    .is_some_and(|t| t.at.elapsed() >= Duration::from_secs(4))
                {
                    this.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub fn project(&self) -> Option<&ProjectState> {
        self.projects.get(self.p)
    }

    pub fn active_wt_path(&self) -> Option<PathBuf> {
        let ps = self.project()?;
        ps.project
            .worktrees
            .get(ps.active_wt)
            .map(|w| w.path.clone())
    }

    pub fn active_branch(&self) -> String {
        self.project()
            .and_then(|ps| ps.project.worktrees.get(ps.active_wt))
            .map(|w| w.branch.clone())
            .unwrap_or_default()
    }

    pub fn wt(&self) -> Option<&WtState> {
        self.wts.get(&self.active_wt_path()?)
    }

    pub fn wt_mut(&mut self) -> Option<&mut WtState> {
        let p = self.active_wt_path()?;
        self.wts.get_mut(&p)
    }

    pub fn brain_handle(&self, project: usize) -> Option<Arc<BrainHandle>> {
        match &self.projects.get(project)?.brain {
            BrainState::Ready { handle, .. } => Some(handle.clone()),
            _ => None,
        }
    }

    fn save_prefs(&self) {
        self.store.set("layout", &self.prefs);
    }

    pub fn sizes(&self) -> (f32, f32, f32) {
        (
            if self.prefs.show_side {
                self.prefs.side_w
            } else {
                0.
            },
            if self.prefs.show_right {
                self.prefs.right_w
            } else {
                0.
            },
            if self.prefs.show_term {
                self.prefs.term_h
            } else {
                0.
            },
        )
    }

    // ---------- projects & worktrees ----------

    pub fn scan_project(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(ps) = self.projects.get_mut(i) else {
            return;
        };
        if ps.scanning {
            return;
        }
        ps.scanning = true;
        let mut project = ps.project.clone();
        let task = cx.background_spawn(async move {
            let prs = insyde_core::forge::open_prs(&project.root);
            project.scan(&prs);
            let base_sha = insyde_core::git::head_sha(&project.root, &project.base);
            (project, base_sha)
        });
        cx.spawn(async move |this, cx| {
            let (project, base_sha) = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(ps) = this.projects.get_mut(i) {
                    let active_path = ps
                        .project
                        .worktrees
                        .get(ps.active_wt)
                        .map(|w| w.path.clone());
                    ps.project.worktrees = project.worktrees;
                    ps.project.stack = project.stack;
                    ps.scanning = false;
                    // Keep the selection on the same worktree; restore the saved one on first scan.
                    let saved: Option<PathBuf> = this
                        .store
                        .get(&format!("active_wt:{}", ps.project.root.display()));
                    let want = active_path.or(saved);
                    ps.active_wt = want
                        .and_then(|p| ps.project.worktrees.iter().position(|w| w.path == p))
                        .unwrap_or(0);
                }
                if i == this.p {
                    this.ensure_wt(window_less(), cx);
                    this.refresh_wt_details(cx);
                    // Keep the brain in step with the base branch when asked to.
                    let stale = matches!(
                        (&this.projects[i].brain, &base_sha),
                        (BrainState::Ready { handle, .. }, Some(sha)) if handle.graph.read().sha != *sha
                    );
                    if stale && insyde_core::settings::get().brain_auto_update {
                        this.log("Base branch moved: updating the Project Brain");
                        this.build_brain(cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn load_brain(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(ps) = self.projects.get(i) else {
            return;
        };
        let (root, base) = (ps.project.root.clone(), ps.project.base.clone());
        if !Brain::exists(&root) {
            return;
        }
        let task = cx.background_spawn(async move {
            let brain = Brain::open(&root, &base).ok()?;
            let graph = brain.load().ok()?;
            let built = graph.built_at;
            Some((
                Arc::new(BrainHandle {
                    brain,
                    graph: parking_lot::RwLock::new(graph),
                }),
                built,
            ))
        });
        cx.spawn(async move |this, cx| {
            if let Some((handle, updated)) = task.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(ps) = this.projects.get_mut(i) {
                        ps.brain = BrainState::Ready { handle, updated };
                    }
                    this.push_brain_to_chats(i, cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn build_brain(&mut self, cx: &mut Context<Self>) {
        let i = self.p;
        let Some(ps) = self.projects.get_mut(i) else {
            return;
        };
        if matches!(ps.brain, BrainState::Building { .. }) {
            return;
        }
        let (root, base) = (ps.project.root.clone(), ps.project.base.clone());
        ps.brain = BrainState::Building {
            pct: 0,
            label: format!("Reading {base}…"),
        };
        self.brain_view = None;
        let (tx, rx) = flume::unbounded::<(u8, String)>();
        let task = cx.background_spawn(async move {
            let brain = Brain::open(&root, &base)?;
            let progress: insyde_core::brain::Progress = Arc::new(move |p, l| {
                let _ = tx.send((p, l));
            });
            let graph = brain.build(progress)?;
            anyhow::Ok(Arc::new(BrainHandle {
                brain,
                graph: parking_lot::RwLock::new(graph),
            }))
        });
        cx.spawn(async move |this, cx| {
            while let Ok((pct, label)) = rx.recv_async().await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(ps) = this.projects.get_mut(i) {
                        ps.brain = BrainState::Building { pct, label };
                    }
                    cx.notify();
                });
            }
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(handle) => {
                        let n = handle.node_count();
                        if let Some(ps) = this.projects.get_mut(i) {
                            ps.brain = BrainState::Ready {
                                handle,
                                updated: insyde_core::store::now(),
                            };
                        }
                        this.push_brain_to_chats(i, cx);
                        this.toast(format!("Project Brain ready · {n} notes"), false, cx);
                    }
                    Err(e) => {
                        if let Some(ps) = this.projects.get_mut(i) {
                            ps.brain = BrainState::None;
                        }
                        this.toast(format!("Brain build failed: {e}"), true, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn push_brain_to_chats(&mut self, i: usize, cx: &mut Context<Self>) {
        let handle = self.brain_handle(i);
        let Some(ps) = self.projects.get(i) else {
            return;
        };
        let paths: Vec<PathBuf> = ps
            .project
            .worktrees
            .iter()
            .map(|w| w.path.clone())
            .collect();
        for p in paths {
            if let Some(ws) = self.wts.get(&p) {
                for t in &ws.tabs {
                    if let TabView::Chat(c) = &t.view {
                        c.update(cx, |c, _| c.set_brain(handle.clone()));
                    }
                }
            }
        }
    }

    pub fn open_brain(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(handle) = self.brain_handle(self.p) else {
            return;
        };
        let (name, updated) = match self.project() {
            Some(ProjectState {
                project,
                brain: BrainState::Ready { updated, .. },
                ..
            }) => (project.name.clone(), *updated),
            _ => return,
        };
        let view = cx.new(|cx| {
            BrainView::new(
                handle,
                name,
                format!("Updated {}", insyde_core::git::ago(updated)),
                window,
                cx,
            )
        });
        self._subs.push(cx.subscribe_in(
            &view,
            window,
            |this, _, ev: &BrainEvent, _, cx| match ev {
                BrainEvent::Close => {
                    this.brain_view = None;
                    cx.notify();
                }
                BrainEvent::Update => this.build_brain(cx),
            },
        ));
        self.brain_view = Some(view);
        cx.notify();
    }

    /// Create UI state for the active worktree (restoring saved sessions).
    pub fn ensure_wt(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let Some(path) = self.active_wt_path() else {
            return;
        };
        if self.wts.contains_key(&path) {
            return;
        }
        // Tabs need a window; without one, defer the whole setup to the next render.
        if window.is_none() {
            self.pending_default_tab = true;
            cx.notify();
            return;
        }
        let ws = WtState {
            tabs: vec![],
            active: 0,
            panes: vec![],
            pr: None,
            files: vec![],
            diff_sel: None,
            patch: None,
            editor: None,
            run: None,
            comments: vec![],
            draft: None,
            loading_pr: false,
        };
        let mut ws = ws;
        ws.comments = self.store.pending_comments(&path);
        self.wts.insert(path.clone(), ws);
        // The first terminal runs the worktree's setup command, if any.
        let setup = self.pending_setup.remove(&path);
        self.add_pane(setup, cx);
        let n = insyde_core::settings::get().default_terminals.clamp(1, 6);
        for _ in 1..n {
            self.add_pane(None, cx);
        }
        let rows = self.store.open_sessions(&path).unwrap_or_default();
        let mut restored = false;
        if let Some(window) = window {
            for r in rows {
                if let Some(spec) = AgentSpec::by_key(&r.agent) {
                    self.open_chat(spec.id, Some(r), None, window, cx);
                    restored = true;
                }
            }
            if !restored && !self.suppress_default_tab {
                self.open_chat(crate::prefs::default_agent(), None, None, window, cx);
            }
            if let Some(ws) = self.wts.get_mut(&path) {
                ws.active = 0;
            }
        } else {
            self.pending_default_tab = true;
        }
    }

    pub fn refresh_wt_details(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.active_wt_path() else {
            return;
        };
        let base = self
            .project()
            .map(|p| p.project.base.clone())
            .unwrap_or_else(|| "main".into());
        if let Some(ws) = self.wts.get_mut(&path) {
            ws.loading_pr = true;
        }
        let p2 = path.clone();
        let task = cx.background_spawn(async move {
            let files = insyde_core::git::changed_files(&p2, &base);
            let ab = insyde_core::git::ahead_behind(&p2);
            let pr = insyde_core::forge::pr_for(&p2);
            let run = insyde_core::project::detect_run(&p2);
            (files, pr, ab, run)
        });
        cx.spawn(async move |this, cx| {
            let (files, pr, ab, run) = task.await;
            let _ = this.update(cx, |this, cx| {
                match ab {
                    Some(v) => {
                        this.ahead_behind.insert(path.clone(), v);
                    }
                    None => {
                        this.ahead_behind.remove(&path);
                    }
                }
                if let Some(ws) = this.wts.get_mut(&path) {
                    ws.files = files;
                    ws.run = run;
                    ws.pr = pr;
                    ws.loading_pr = false;
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn select_project(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.projects.is_empty() {
            return;
        }
        let n = self.projects.len();
        self.p = (i + n) % n;
        self.store.set("active_project", &self.p);
        self.menu_open = false;
        self.handoff = None;
        self.ensure_wt(Some(window), cx);
        self.refresh_wt_details(cx);
        self.scan_project(self.p, cx);
        cx.notify();
    }

    pub fn select_wt(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ps) = self.projects.get_mut(self.p) else {
            return;
        };
        if i >= ps.project.worktrees.len() {
            return;
        }
        ps.active_wt = i;
        let root = ps.project.root.clone();
        let path = ps.project.worktrees[i].path.clone();
        self.store
            .set(&format!("active_wt:{}", root.display()), &path);
        self.menu_open = false;
        self.handoff = None;
        self.confirm_delete = None;
        self.ensure_wt(Some(window), cx);
        self.refresh_wt_details(cx);
        if let Some(ws) = self.wts.get_mut(&path)
            && let Some(TabView::Chat(c)) = ws.tabs.get(ws.active).map(|t| &t.view)
        {
            c.update(cx, |c, _| c.unseen = false);
        }
        cx.notify();
    }

    pub fn add_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.connect_form = None;
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open repository".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.open_project(path, None, window, cx)
            });
        })
        .detach();
    }

    /// "Connect over SSH" box: `user@host:/path/to/repo`.
    pub fn open_connect_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("user@host:/path/to/repo"));
        self._subs.push(
            cx.subscribe_in(&input, window, |this, s, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    let target = s.read(cx).value().to_string();
                    match insyde_core::remote::parse_target(&target) {
                        Some(path) => {
                            this.connect_form = None;
                            this.open_project(path, None, window, cx);
                        }
                        None => this.toast("Use user@host:/path/to/repo", true, cx),
                    }
                }
            }),
        );
        input.update(cx, |s, cx| s.focus(window, cx));
        self.connect_form = Some(input);
        cx.notify();
    }

    pub fn start_new_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Task or branch, e.g. fix/login redirect")
        });
        self._subs.push(cx.subscribe_in(
            &input,
            window,
            |this, s, ev: &InputEvent, _window, cx| match ev {
                InputEvent::PressEnter { .. } => {
                    let title = s.read(cx).value().to_string();
                    this.new_wt = None;
                    if !title.trim().is_empty() {
                        this.create_worktree(title, None, cx);
                    }
                    cx.notify();
                }
                InputEvent::Blur => {
                    this.new_wt = None;
                    cx.notify();
                }
                _ => {}
            },
        ));
        input.update(cx, |s, cx| s.focus(window, cx));
        self.new_wt = Some(input);
        cx.notify();
    }

    pub(crate) fn create_worktree(
        &mut self,
        title: String,
        reply: Option<insyde_core::rpc::Call>,
        cx: &mut Context<Self>,
    ) {
        let Some(ps) = self.project() else {
            if let Some(r) = reply {
                r.err("no project open");
            }
            return;
        };
        let (root, base, i) = (ps.project.root.clone(), ps.project.base.clone(), self.p);
        let settings = insyde_core::settings::get();
        let branch = insyde_core::git::slugify_branch_with(&title, &settings.branch_prefix);
        self.toast(format!("Creating worktree {branch}…"), false, cx);
        let b2 = branch.clone();
        let copies = settings.copy_list();
        let task = cx.background_spawn(async move {
            let path = insyde_core::git::add_worktree(&root, &b2, &base)?;
            // Bring over untracked local files (e.g. .env) that the task will need.
            for f in copies {
                if let Ok(bytes) = insyde_core::remote::read_file(&root.join(&f)) {
                    let _ = insyde_core::remote::write_file(&path.join(&f), &bytes);
                }
            }
            anyhow::Ok(path)
        });
        cx.spawn(async move |this, cx| {
            let res = task.await;
            let _ = this.update(cx, |this, cx| match res {
                Ok(path) => {
                    this.toast(format!("Worktree {branch} ready"), false, cx);
                    let setup = insyde_core::settings::get().setup_command;
                    if !setup.trim().is_empty() {
                        this.pending_setup.insert(path.clone(), setup);
                    }
                    let root = this.projects.get(i).map(|p| p.project.root.clone());
                    if let Some(root) = root {
                        this.store
                            .set(&format!("active_wt:{}", root.display()), &path);
                    }
                    if let Some(ps) = this.projects.get_mut(i) {
                        ps.active_wt = usize::MAX; // select after scan via saved path
                    }
                    this.scan_project(i, cx);
                    if let Some(r) = reply {
                        r.ok(serde_json::json!({ "path": path, "branch": branch }));
                    }
                }
                Err(e) => {
                    if let Some(r) = reply {
                        r.err(e.to_string());
                    }
                    this.toast(format!("Couldn't create worktree: {e}"), true, cx)
                }
            });
        })
        .detach();
    }

    pub fn delete_worktree(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(ps) = self.project() else { return };
        let (root, i) = (ps.project.root.clone(), self.p);
        self.confirm_delete = None;
        self.wts.remove(&path);
        let p2 = path.clone();
        let task = cx
            .background_spawn(async move { insyde_core::git::remove_worktree(&root, &p2, false) });
        cx.spawn(async move |this, cx| {
            let res = task.await;
            let _ = this.update(cx, |this, cx| {
                match res {
                    Ok(()) => this.toast(format!("Removed worktree {}", path.display()), false, cx),
                    Err(e) => this.toast(format!("Kept worktree: {e}"), true, cx),
                }
                if let Some(ps) = this.projects.get_mut(i) {
                    ps.active_wt = 0;
                }
                this.scan_project(i, cx);
            });
        })
        .detach();
    }

    // ---------- tabs ----------

    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn others_for(&self, cx: &App) -> String {
        let Some(ws) = self.wt() else {
            return String::new();
        };
        let active_id = ws.tabs.get(ws.active).map(|t| t.id);
        let mut names: Vec<&str> = ws
            .tabs
            .iter()
            .filter(|t| Some(t.id) != active_id && t.is_agent())
            .map(|t| AgentSpec::get(t.agent).name)
            .collect();
        names.sort();
        names.dedup();
        let _ = cx;
        names.join(", ")
    }

    pub fn open_chat(
        &mut self,
        agent: AgentId,
        resume: Option<insyde_core::store::SessionRow>,
        carried: Option<Carried>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.active_wt_path() else {
            return;
        };
        let spec = AgentSpec::get(agent);
        let branch = self.active_branch();
        let project = self
            .project()
            .map(|p| p.project.name.clone())
            .unwrap_or_default();
        let brain = self.brain_handle(self.p);
        let policy: Policy = crate::prefs::policy();
        let title = resume
            .as_ref()
            .map(|r| r.title.clone())
            .or_else(|| {
                carried
                    .as_ref()
                    .map(|c| c.from.split(" · ").nth(1).unwrap_or(spec.name).to_string())
            })
            .unwrap_or_else(|| spec.name.to_string());
        let store = self.store.clone();
        let view = cx.new(|cx| {
            ChatView::new(
                ChatInit {
                    agent,
                    title,
                    worktree: path.clone(),
                    branch,
                    project,
                    store,
                    brain,
                    carried,
                    resume,
                    policy,
                },
                window,
                cx,
            )
        });
        self._subs.push(cx.subscribe_in(
            &view,
            window,
            |this, chat, ev: &ChatEvent, window, cx| match ev {
                ChatEvent::Status => {
                    // An agent may have edited the open file: reload it unless you have unsaved edits.
                    if !chat.read(cx).is_running()
                        && let Some(ed) = this.wt().and_then(|w| w.editor.clone())
                    {
                        ed.update(cx, |e, cx| e.reload_if_clean(window, cx));
                    }
                    // A turn that finishes in the tab you're looking at is already reviewed.
                    if this.active_chat().as_ref() == Some(chat) && chat.read(cx).unseen {
                        chat.update(cx, |c, _| c.unseen = false);
                    }
                    cx.notify()
                }
                ChatEvent::OpenDiff => {
                    this.right_tab = RightTab::Diff;
                    this.prefs.show_right = true;
                    this.refresh_wt_details(cx);
                    cx.notify();
                }
                ChatEvent::Handoff => this.open_handoff(window, cx),
                ChatEvent::CreateBrain => this.build_brain(cx),
                ChatEvent::TurnDone | ChatEvent::NeedsPermission => {
                    let s = insyde_core::settings::get();
                    let wanted = if matches!(ev, ChatEvent::TurnDone) {
                        s.notify_turn_done
                    } else {
                        s.notify_permission
                    };
                    if wanted && (!s.notify_only_background || !window.is_window_active()) {
                        let c = chat.read(cx);
                        let agent = AgentSpec::get(c.agent).name;
                        let (title, body) = if matches!(ev, ChatEvent::TurnDone) {
                            (
                                format!("{agent} finished"),
                                c.last_reply().chars().take(140).collect::<String>(),
                            )
                        } else {
                            (
                                format!("{agent} needs your approval"),
                                format!("{} · {}", c.branch, c.title),
                            )
                        };
                        cx.show_system_notification(gpui::SystemNotification {
                            tag: format!("insyde-{}", c.title).into(),
                            title: title.into(),
                            body: body.into(),
                            actions: vec![],
                        });
                    }
                }
            },
        ));
        let id = self.next_id();
        if let Some(ws) = self.wts.get_mut(&path) {
            ws.tabs.push(AgentTab {
                id,
                agent,
                view: TabView::Chat(view.clone()),
            });
            ws.active = ws.tabs.len() - 1;
        }
        let others = self.others_for(cx);
        view.update(cx, |c, cx| {
            c.others = others;
            c.focus_composer(window, cx);
        });
        cx.notify();
    }

    /// Run an agent's own terminal UI (or a plain shell) as a tab.
    pub fn open_tui(&mut self, agent: AgentId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.active_wt_path() else {
            return;
        };
        let spec = AgentSpec::get(agent);
        // The default permission level from settings becomes the CLI's own flag
        // (e.g. `claude --dangerously-skip-permissions`).
        let perms = insyde_core::agents::tui_permission_args(
            spec.key,
            insyde_core::settings::get().approval,
        );
        let program = spec.tui.map(|c| {
            (
                c.program.to_string(),
                c.args
                    .iter()
                    .chain(perms)
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>(),
            )
        });
        if let Some((p, _)) = &program
            && insyde_core::agents::which(p).is_none()
        {
            self.toast(
                format!("{} isn't installed ({p} not found on PATH)", spec.name),
                true,
                cx,
            );
            return;
        }
        let program = program.map(|(p, a)| {
            (
                insyde_core::agents::which(&p)
                    .map(|x| x.to_string_lossy().into_owned())
                    .unwrap_or(p),
                a,
            )
        });
        let view = cx.new(|cx| TerminalView::new(path.clone(), program, self.term_env(&path), cx));
        self._subs
            .push(cx.subscribe(&view, |_, _, ev: &TerminalEvent, cx| {
                if !matches!(ev, TerminalEvent::Activity) {
                    cx.notify();
                }
            }));
        let id = self.next_id();
        if let Some(ws) = self.wts.get_mut(&path) {
            ws.tabs.push(AgentTab {
                id,
                agent,
                view: TabView::Term(view.clone()),
            });
            ws.active = ws.tabs.len() - 1;
        }
        view.update(cx, |v, cx| v.focus_handle(cx).focus(window, cx));
        cx.notify();
    }

    pub fn add_agent(
        &mut self,
        idx: usize,
        terminal_ui: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu_open = false;
        let Some(spec) = insyde_core::agents::AGENTS.get(idx) else {
            return;
        };
        match spec.id {
            AgentId::Terminal => self.open_tui(AgentId::Terminal, window, cx),
            AgentId::Browser => {
                let url = self
                    .dev_url()
                    .unwrap_or_else(|| "http://localhost:3000".into());
                cx.open_url(&url);
                self.toast(format!("Opened {url} in your browser"), false, cx);
            }
            id => {
                // ⌘ flips the default chosen in settings (chat UI vs. the agent's own TUI).
                let prefer_terminal = insyde_core::settings::get().open_agents_as
                    == insyde_core::settings::OpenAs::Terminal;
                let chat = spec.acp.is_some() && (terminal_ui == prefer_terminal);
                if chat {
                    self.open_chat(id, None, None, window, cx)
                } else {
                    self.open_tui(id, window, cx)
                }
            }
        }
    }

    pub fn close_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let Some(ws) = self.wt_mut() else { return };
        if ws.tabs.len() <= 1 {
            return;
        }
        if let Some(i) = ws.tabs.iter().position(|t| t.id == id) {
            if let TabView::File(e) = &ws.tabs[i].view
                && e.read(cx).dirty
            {
                let rel = e.read(cx).rel.clone();
                ws.active = i;
                self.toast(
                    format!("Unsaved changes in {rel}: save or revert first"),
                    true,
                    cx,
                );
                return;
            }
            let tab = ws.tabs.remove(i);
            if let TabView::Chat(c) = &tab.view
                && let Some(row) = c.read(cx).session_row()
            {
                store.close_session(row);
            }
            if ws.active >= ws.tabs.len() || i <= ws.active {
                ws.active = ws
                    .active
                    .saturating_sub(if i <= ws.active && ws.active > 0 {
                        1
                    } else {
                        0
                    })
                    .min(ws.tabs.len() - 1);
            }
        }
        cx.notify();
    }

    pub fn active_chat(&self) -> Option<Entity<ChatView>> {
        let ws = self.wt()?;
        match &ws.tabs.get(ws.active)?.view {
            TabView::Chat(c) => Some(c.clone()),
            _ => None,
        }
    }

    // ---------- terminals ----------

    fn term_env(&self, path: &Path) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert(
            "INSYDE_WORKTREE".into(),
            path.to_string_lossy().into_owned(),
        );
        env.insert(
            "INSYDE_SOCKET".into(),
            insyde_core::rpc::socket_path()
                .to_string_lossy()
                .into_owned(),
        );
        env.extend(insyde_core::settings::get().env_pairs());
        env
    }

    pub fn add_pane(&mut self, command: Option<String>, cx: &mut Context<Self>) {
        let Some(path) = self.active_wt_path() else {
            return;
        };
        let env = self.term_env(&path);
        let view = cx.new(|cx| TerminalView::new(path.clone(), None, env, cx));
        if let Some(cmd) = command {
            let v = view.clone();
            cx.spawn(async move |_, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(350))
                    .await;
                v.update(cx, |v, _| v.send_text(&format!("{cmd}\r")));
            })
            .detach();
        }
        self._subs
            .push(cx.subscribe(&view, |_, _, ev: &TerminalEvent, cx| {
                if !matches!(ev, TerminalEvent::Activity) {
                    cx.notify();
                }
            }));
        if let Some(ws) = self.wts.get_mut(&path) {
            ws.panes.push(Pane { view, frac: 1. });
        }
        self.prefs.show_term = true;
        cx.notify();
    }

    pub fn close_pane(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(ws) = self.wt_mut()
            && i < ws.panes.len()
        {
            ws.panes.remove(i);
        }
        cx.notify();
    }

    /// Open a terminal pane in its own window (design: "Pop out to another display").
    pub fn pop_out(&mut self, view: Entity<TerminalView>, cx: &mut Context<Self>) {
        let title: SharedString = view.read(cx).title.clone();
        let opts = gpui::WindowOptions {
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(title),
                appears_transparent: false,
                traffic_light_position: None,
            }),
            window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::centered(
                None,
                gpui::size(px(820.), px(520.)),
                cx,
            ))),
            ..Default::default()
        };
        let _ = cx.open_window(opts, move |_, cx| cx.new(|_| crate::PopOut { view }));
    }

    /// Best "run" command for the project, plus its button label.
    /// Cached run command of the active worktree (detected in the background).
    /// The Run button's `(command, label)`: the setting, else the detected one.
    pub fn run_command(&self) -> Option<(String, String)> {
        insyde_core::project::with_override(self.detected_run())
    }

    /// What the active worktree's package manager or build tool would run.
    pub fn detected_run(&self) -> Option<(String, String)> {
        self.wt().and_then(|w| w.run.clone())
    }

    /// Open or close the Run button's menu; the input starts focused.
    pub fn toggle_run_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.run_menu.take().is_some() {
            cx.notify();
            return;
        }
        self.handoff = None;
        self.history_open = false;
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Any command, e.g. pnpm test"));
        self._subs.push(
            cx.subscribe_in(&input, window, |this, s, ev: &InputEvent, _, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    let cmd = s.read(cx).value().trim().to_string();
                    if !cmd.is_empty() {
                        this.save_quick(&cmd);
                        this.run_now(cmd, cx);
                    }
                }
            }),
        );
        input.update(cx, |st, cx| st.focus(window, cx));
        self.run_menu = Some(input);
        cx.notify();
    }

    /// Run `cmd` in a new terminal and close the Run menu.
    pub fn run_now(&mut self, cmd: String, cx: &mut Context<Self>) {
        self.run_menu = None;
        self.add_pane(Some(cmd), cx);
    }

    /// Remember `cmd` at the top of the Run menu's list.
    pub fn save_quick(&mut self, cmd: &str) {
        insyde_core::settings::update(|s| {
            let mut list = s.quick_list();
            list.retain(|c| c != cmd);
            list.insert(0, cmd.to_string());
            list.truncate(12);
            s.quick_commands = list.join("\n");
        });
    }

    pub fn remove_quick(&mut self, cmd: &str, cx: &mut Context<Self>) {
        insyde_core::settings::update(|s| {
            let list: Vec<String> = s.quick_list().into_iter().filter(|c| c != cmd).collect();
            s.quick_commands = list.join("\n");
        });
        cx.notify();
    }

    /// Make `cmd` the Run button's command; the detected command clears the override.
    pub fn set_run_button(&mut self, cmd: &str, cx: &mut Context<Self>) {
        let detected = self.detected_run().map(|r| r.0);
        let v = if detected.as_deref() == Some(cmd) {
            String::new()
        } else {
            cmd.to_string()
        };
        insyde_core::settings::update(|s| s.run_command = v);
        cx.notify();
    }

    /// A dev-server URL for the "Browser" entry.
    fn dev_url(&self) -> Option<String> {
        None
    }

    // ---------- search ----------

    fn run_search(&mut self, q: String, cx: &mut Context<Self>) {
        let Some(root) = self.active_wt_path() else {
            return;
        };
        if q.trim().len() < 2 {
            self.search_results.clear();
            cx.notify();
            return;
        }
        let task = cx.background_spawn(async move { crate::search::grep(&root, &q, 200) });
        cx.spawn(async move |this, cx| {
            let res = task.await;
            let _ = this.update(cx, |this, cx| {
                this.search_results = res;
                cx.notify();
            });
        })
        .detach();
    }

    pub fn open_file(
        &mut self,
        rel: String,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.active_wt_path() else {
            return;
        };
        // Never drop unsaved work when switching files.
        if let Some(ed) = self.wt().and_then(|w| w.editor.clone()) {
            let (same, dirty, cur) = {
                let e = ed.read(cx);
                (e.rel == rel, e.dirty, e.rel.clone())
            };
            if dirty && !same && insyde_core::settings::get().editor_autosave {
                ed.update(cx, |e, cx| e.save(cx));
                if ed.read(cx).dirty {
                    return; // save failed: keep the edits open (the error was shown)
                }
            } else if dirty {
                self.right_tab = RightTab::Editor;
                if !same {
                    self.toast(
                        format!("Unsaved changes in {cur}: save or revert first"),
                        true,
                        cx,
                    );
                }
                cx.notify();
                return;
            }
        }
        let ed = cx.new(|cx| crate::editor::FileEditor::open(root, rel, line, window, cx));
        self._subs.push(cx.subscribe(
            &ed,
            |this, _, ev: &crate::editor::EditorEvent, cx| match ev {
                crate::editor::EditorEvent::Saved(p) => {
                    this.log(format!("Saved {p}"));
                    this.refresh_wt_details(cx);
                }
                crate::editor::EditorEvent::Error(e) => this.toast(e.clone(), true, cx),
            },
        ));
        if let Some(ws) = self.wt_mut() {
            ws.editor = Some(ed);
        }
        self.right_tab = RightTab::Editor;
        self.prefs.show_right = true;
        // Editing needs room: widen the panel once.
        if self.prefs.right_w < 520. {
            self.prefs.right_w = 520.;
            self.save_prefs();
        }
        cx.notify();
    }

    pub fn save_file(&mut self, cx: &mut Context<Self>) {
        let tab_editor = self
            .wt()
            .and_then(|w| match w.tabs.get(w.active).map(|t| &t.view) {
                Some(TabView::File(e)) => Some(e.clone()),
                _ => None,
            });
        if let Some(ed) = tab_editor.or_else(|| self.wt().and_then(|w| w.editor.clone())) {
            ed.update(cx, |e, cx| e.save(cx));
        }
    }

    pub fn select_diff_file(&mut self, rel: String, cx: &mut Context<Self>) {
        let Some(root) = self.active_wt_path() else {
            return;
        };
        let base = self
            .project()
            .map(|p| p.project.base.clone())
            .unwrap_or_default();
        if let Some(ws) = self.wt_mut() {
            ws.diff_sel = Some(rel.clone());
            ws.patch = None;
        }
        let task = cx.background_spawn(async move {
            insyde_core::git::annotate_patch(&insyde_core::git::file_patch(&root, &base, &rel))
        });
        cx.spawn(async move |this, cx| {
            let lines = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(ws) = this.wt_mut() {
                    ws.patch = Some(Arc::new(lines));
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ---------- review comments ----------

    pub fn start_comment(
        &mut self,
        line: insyde_core::git::PatchLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.wt().and_then(|w| w.diff_sel.clone()) else {
            return;
        };
        let (n, side) = match (line.new, line.old) {
            (Some(n), _) => (n as i64, "new"),
            (None, Some(o)) => (o as i64, "old"),
            _ => return,
        };
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Comment for the agent… (Enter to add)")
        });
        self._subs.push(
            cx.subscribe_in(&input, window, |this, s, ev: &InputEvent, _, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    let body = s.read(cx).value().to_string();
                    this.add_comment(body, cx);
                }
            }),
        );
        input.update(cx, |s, cx| s.focus(window, cx));
        let code = line.text.get(1..).unwrap_or("").to_string();
        if let Some(ws) = self.wt_mut() {
            ws.draft = Some((path, n, side.to_string(), code, input));
        }
        cx.notify();
    }

    pub fn add_comment(&mut self, body: String, cx: &mut Context<Self>) {
        let Some(worktree) = self.active_wt_path() else {
            return;
        };
        let store = self.store.clone();
        let Some(ws) = self.wt_mut() else {
            return;
        };
        let Some((path, line, side, code, _)) = ws.draft.take() else {
            return;
        };
        if !body.trim().is_empty() {
            let mut c = insyde_core::store::Comment {
                id: 0,
                worktree,
                path,
                line,
                side,
                code,
                body,
            };
            c.id = store.add_comment(&c);
            ws.comments.push(c);
        }
        cx.notify();
    }

    pub fn delete_comment(&mut self, id: i64, cx: &mut Context<Self>) {
        self.store.delete_comment(id);
        if let Some(ws) = self.wt_mut() {
            ws.comments.retain(|c| c.id != id);
        }
        cx.notify();
    }

    /// Send all pending comments as one prompt to the active chat (starting one if needed).
    pub fn send_comments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let comments = self.wt().map(|w| w.comments.clone()).unwrap_or_default();
        if comments.is_empty() {
            return;
        }
        let chat = match self.active_chat() {
            Some(c) => c,
            None => {
                self.open_chat(crate::prefs::default_agent(), None, None, window, cx);
                match self.active_chat() {
                    Some(c) => c,
                    None => return,
                }
            }
        };
        let prompt = insyde_core::store::Comment::prompt(&comments);
        chat.update(cx, |c, cx| c.send_text(prompt, window, cx));
        let ids: Vec<i64> = comments.iter().map(|c| c.id).collect();
        self.store.mark_comments_sent(&ids);
        if let Some(ws) = self.wt_mut() {
            ws.comments.clear();
        }
        self.toast(
            format!(
                "Sent {} comment{} to the agent",
                ids.len(),
                if ids.len() == 1 { "" } else { "s" }
            ),
            false,
            cx,
        );
    }

    // ---------- PRs ----------

    pub fn create_or_open_pr(&mut self, cx: &mut Context<Self>) {
        if let Some(url) = self.wt().and_then(|w| w.pr.as_ref()).map(|p| p.url.clone()) {
            cx.open_url(&url);
            return;
        }
        let Some(path) = self.active_wt_path() else {
            return;
        };
        let base = self
            .project()
            .map(|p| p.project.base.clone())
            .unwrap_or_default();
        if self.active_branch() == base {
            self.toast(
                format!("You're on {base}. Create a worktree for the task first."),
                true,
                cx,
            );
            return;
        }
        self.toast("Pushing branch and opening a draft PR…", false, cx);
        let task = cx.background_spawn(async move {
            insyde_core::forge::create_pr(&path, &base, insyde_core::settings::get().pr_draft)
        });
        cx.spawn(async move |this, cx| {
            let res = task.await;
            let _ = this.update(cx, |this, cx| {
                match res {
                    Ok(url) => {
                        this.toast(format!("Opened {url}"), false, cx);
                        this.right_tab = RightTab::Checks;
                    }
                    Err(e) => this.toast(format!("PR failed: {e}"), true, cx),
                }
                this.refresh_wt_details(cx);
                this.scan_project(this.p, cx);
            });
        })
        .detach();
    }

    pub fn rerun_checks(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.active_wt_path() else {
            return;
        };
        let task = cx.background_spawn(async move { insyde_core::forge::rerun_failed(&path) });
        cx.spawn(async move |this, cx| {
            let ok = task.await;
            let _ = this.update(cx, |this, cx| {
                this.toast(
                    if ok {
                        "Re-running failed checks"
                    } else {
                        "Nothing to re-run (or gh unavailable)"
                    },
                    !ok,
                    cx,
                );
                this.refresh_wt_details(cx);
            });
        })
        .detach();
    }

    // ---------- hand-off ----------

    pub fn open_handoff(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.handoff.is_some() {
            self.handoff = None;
            cx.notify();
            return;
        }
        self.menu_open = false;
        let Some(chat) = self.active_chat() else {
            self.toast("Hand off works from an agent chat tab", true, cx);
            return;
        };
        let (summary, tasks, paths, a, r) = chat.read(cx).handoff_material();
        let terms: String = self
            .wt()
            .map(|ws| {
                ws.panes
                    .iter()
                    .map(|p| {
                        let v = p.view.read(cx);
                        format!("### {} ({})\n```\n{}\n```\n", v.title, v.sub, v.tail(200))
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let term_names: Vec<String> = self
            .wt()
            .map(|ws| {
                ws.panes
                    .iter()
                    .map(|p| p.view.read(cx).title.to_string())
                    .collect()
            })
            .unwrap_or_default();
        let brain = self
            .brain_handle(self.p)
            .map(|b| {
                b.digest(
                    &self
                        .project()
                        .map(|p| p.project.name.clone())
                        .unwrap_or_default(),
                    &summary,
                )
            })
            .unwrap_or_default();
        let current = self
            .wt()
            .and_then(|ws| ws.tabs.get(ws.active))
            .map(|t| t.agent);
        let target = insyde_core::agents::AGENTS
            .iter()
            .position(|s| Some(s.id) != current && s.acp.is_some() && s.id != AgentId::Super)
            .unwrap_or(1);
        let tok = |s: &str| s.len() as f64 / 4.;
        let subs = [
            "Goals, decisions and what was tried".to_string(),
            if paths.is_empty() {
                "No file changes yet".into()
            } else {
                format!(
                    "{} · +{a} −{r}",
                    paths
                        .iter()
                        .take(2)
                        .map(|p| p.rsplit('/').next().unwrap_or(p))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
            if term_names.is_empty() {
                "No terminals".into()
            } else {
                format!("{} · last 200 lines", term_names.join(", "))
            },
            if tasks.is_empty() {
                "No open plan items".into()
            } else {
                tasks
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" · ")
            },
            if brain.is_empty() {
                "No brain yet for this project".into()
            } else {
                "Linked notes for this task".into()
            },
        ];
        let mut h = Handoff {
            target,
            opts: [
                true,
                true,
                !terms.is_empty(),
                !tasks.is_empty(),
                !brain.is_empty(),
            ],
            tokens: [
                tok(&summary),
                0.,
                tok(&terms),
                tok(&tasks.join("\n")),
                tok(&brain),
            ],
            subs,
            summary,
            diff: String::new(),
            terms,
            tasks,
            brain,
        };
        h.opts[1] = !paths.is_empty();
        self.handoff = Some(h);
        // The diff can be large; compute it off the UI thread.
        if let Some(path) = self.active_wt_path() {
            let base = self
                .project()
                .map(|p| p.project.base.clone())
                .unwrap_or_default();
            let task =
                cx.background_spawn(
                    async move { insyde_core::git::full_patch(&path, &base, 60_000) },
                );
            cx.spawn(async move |this, cx| {
                let diff = task.await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(h) = &mut this.handoff {
                        h.tokens[1] = diff.len() as f64 / 4.;
                        h.opts[1] = !diff.is_empty();
                        let files = diff.lines().filter(|l| l.starts_with("diff --git")).count();
                        if files > 0 {
                            let (a, r) = diff.lines().fold((0, 0), |(a, r), l| {
                                if l.starts_with('+') && !l.starts_with("+++") {
                                    (a + 1, r)
                                } else if l.starts_with('-') && !l.starts_with("---") {
                                    (a, r + 1)
                                } else {
                                    (a, r)
                                }
                            });
                            h.subs[1] = format!(
                                "{files} file{} in the worktree diff · +{a} −{r}",
                                if files == 1 { "" } else { "s" }
                            );
                        }
                        h.diff = diff;
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    pub fn do_handoff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(h) = self.handoff.take() else { return };
        let Some(chat) = self.active_chat() else {
            return;
        };
        let (from_agent, from_title) = {
            let c = chat.read(cx);
            (AgentSpec::get(c.agent).name, c.title.to_string())
        };
        let labels = [
            "Conversation summary",
            "Changed files & diff",
            "Terminal & test state",
            "Open tasks",
            "Project Brain",
        ];
        let mut context = format!(
            "# Hand-off from {from_agent}\nYou are continuing work started by another agent in this same worktree. Read this context, then continue with the user's next message.\n"
        );
        let mut items = vec![];
        let mut total = 0.;
        let parts = [
            &h.summary,
            &h.diff,
            &h.terms,
            &h.tasks.join("\n- "),
            &h.brain,
        ];
        for i in 0..5 {
            if !h.opts[i] || parts[i].is_empty() {
                continue;
            }
            total += h.tokens[i];
            items.push((labels[i].to_string(), crate::ui::fmt_k(h.tokens[i] * 1.)));
            let body = if i == 1 {
                format!("```diff\n{}\n```", parts[i])
            } else if i == 3 {
                format!("- {}", parts[i])
            } else {
                parts[i].clone()
            };
            context.push_str(&format!("\n## {}\n{}\n", labels[i], body));
        }
        let picking_up = h.tasks.first().cloned().unwrap_or_default();
        let carried = Carried {
            from: format!("{from_agent} · {from_title}"),
            items,
            tokens: format!("{} tokens", crate::ui::fmt_k(total)),
            context,
            picking_up,
        };
        let agent = insyde_core::agents::AGENTS
            .get(h.target)
            .map(|s| s.id)
            .unwrap_or(DEFAULT_AGENT);
        self.open_chat(agent, None, Some(carried), window, cx);
        if let Some(c) = self.active_chat() {
            c.update(cx, |c, _| c.use_brain = !h.opts[4] && c.use_brain);
        }
        cx.notify();
    }

    // ---------- layout & input ----------

    fn on_mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.drag else { return };
        if !e.dragging() {
            self.drag = None;
            self.save_prefs();
            cx.notify();
            return;
        }
        let (x, y) = (f32::from(e.position.x), f32::from(e.position.y));
        match d {
            Drag::Side { x0, w0 } => self.prefs.side_w = (w0 + x - x0).clamp(180., 420.),
            Drag::Right { x0, w0 } => self.prefs.right_w = (w0 - (x - x0)).clamp(240., 900.),
            Drag::Term { y0, h0 } => self.prefs.term_h = (h0 - (y - y0)).clamp(110., 560.),
            Drag::Pane {
                idx,
                x0,
                fr0,
                width,
            } => {
                if let Some(ws) = self.wt_mut() {
                    let tot: f32 = ws.panes.iter().map(|p| p.frac).sum();
                    let d = (x - x0) / width.max(1.) * tot;
                    let pair = fr0.0 + fr0.1;
                    let min = tot * 0.12;
                    let a = (fr0.0 + d).clamp(min, pair - min);
                    if idx + 1 < ws.panes.len() {
                        ws.panes[idx].frac = a;
                        ws.panes[idx + 1].frac = pair - a;
                    }
                }
            }
        }
        cx.notify();
    }

    pub fn start_drag(&mut self, d: Drag) {
        self.drag = Some(d);
    }

    pub fn reset_sizes(&mut self, cx: &mut Context<Self>) {
        let (s, r, t) = (
            self.prefs.show_side,
            self.prefs.show_right,
            self.prefs.show_term,
        );
        self.prefs = LayoutPrefs {
            show_side: s,
            show_right: r,
            show_term: t,
            ..Default::default()
        };
        if let Some(ws) = self.wt_mut() {
            for p in &mut ws.panes {
                p.frac = 1.;
            }
        }
        self.save_prefs();
        cx.notify();
    }

    pub fn is_dragging(&self, which: &str) -> bool {
        matches!(
            (self.drag, which),
            (Some(Drag::Side { .. }), "side")
                | (Some(Drag::Right { .. }), "right")
                | (Some(Drag::Term { .. }), "term")
        ) || matches!((self.drag, which), (Some(Drag::Pane { idx, .. }), w) if w == format!("pane{idx}"))
    }

    /// Two-finger horizontal swipe over the sidebar switches projects.
    fn on_side_wheel(&mut self, e: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let (dx, dy) = match e.delta {
            ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
            ScrollDelta::Lines(l) => (l.x * 20., l.y * 20.),
        };
        if dx.abs() <= dy.abs()
            || self
                .swipe_lock
                .is_some_and(|t| t.elapsed() < Duration::from_millis(520))
            || self.projects.len() < 2
        {
            return;
        }
        self.swipe_acc -= dx;
        if self.swipe_acc.abs() > 110. {
            let dir = if self.swipe_acc > 0. { 1 } else { -1 };
            self.swipe_acc = 0.;
            self.swipe_dx = 0.;
            self.swipe_lock = Some(Instant::now());
            self.select_project(
                (self.p as i64 + dir + self.projects.len() as i64) as usize % self.projects.len(),
                window,
                cx,
            );
        } else {
            self.swipe_dx = (-self.swipe_acc * 0.5).clamp(-70., 70.);
            let acc_now = self.swipe_acc;
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(160))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.swipe_acc == acc_now {
                        this.swipe_acc = 0.;
                        this.swipe_dx = 0.;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        if (self.run_menu.is_some() || self.file_menu.is_some()) && k.key == "escape" {
            self.run_menu = None;
            self.file_menu = None;
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if self.menu_open {
            if k.key == "escape" {
                self.menu_open = false;
                cx.notify();
                cx.stop_propagation();
                return;
            }
            if let Ok(n) = k.key.parse::<usize>()
                && (1..=9).contains(&n)
            {
                self.add_agent(n - 1, k.modifiers.platform, window, cx);
                cx.stop_propagation();
            }
        } else if self.handoff.is_some() && k.key == "escape" {
            self.handoff = None;
            cx.notify();
        }
    }

    pub fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        let dark = cx.theme().is_dark();
        insyde_core::settings::update(|s| {
            s.theme = if dark {
                insyde_core::settings::ThemeChoice::Light
            } else {
                insyde_core::settings::ThemeChoice::Dark
            }
        });
        crate::prefs::apply_theme(cx);
    }

    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_view.is_some() {
            return;
        }
        self.menu_open = false;
        let view =
            cx.new(|cx| crate::settings_view::SettingsView::new(self.store.clone(), window, cx));
        self._subs.push(cx.subscribe(
            &view,
            |this, _, ev: &crate::settings_view::SettingsEvent, cx| match ev {
                crate::settings_view::SettingsEvent::Close => {
                    this.settings_view = None;
                    cx.notify();
                }
                crate::settings_view::SettingsEvent::BrainsCleared => {
                    for ps in &mut this.projects {
                        ps.brain = BrainState::None;
                    }
                    this.brain_view = None;
                    cx.notify();
                }
            },
        ));
        self.settings_view = Some(view);
        cx.notify();
    }

    /// Index of the default agent in the picker (for "+ Agent" and ⌘T).
    pub fn default_agent_index() -> usize {
        let id = crate::prefs::default_agent();
        insyde_core::agents::AGENTS
            .iter()
            .position(|a| a.id == id)
            .unwrap_or(2)
    }

    pub fn counts(&self, cx: &App) -> (usize, usize) {
        let (mut running, mut review) = (0, 0);
        for ws in self.wts.values() {
            for t in &ws.tabs {
                match &t.view {
                    TabView::Chat(c) => {
                        let c = c.read(cx);
                        if c.is_running() {
                            running += 1;
                        }
                        if c.needs_attention() {
                            review += 1;
                        }
                    }
                    TabView::Term(v) => {
                        if v.read(cx).is_busy() {
                            running += 1;
                        }
                    }
                    TabView::File(_) => {}
                }
            }
        }
        (running, review)
    }

    pub fn live_worktrees(&self, cx: &App) -> std::collections::HashSet<PathBuf> {
        self.wts
            .iter()
            .filter(|(_, ws)| ws.tabs.iter().any(|t| t.running(cx)))
            .map(|(p, _)| p.clone())
            .collect()
    }
}

/// Placeholder used where a scan completes without a window handle; the next
/// render creates the worktree's default tab.
fn window_less<'a>() -> Option<&'a mut Window> {
    None
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        // Keyboard shortcuts need a focused element inside the workspace: when the
        // focused view goes away (closing Settings, a popover…), take focus back.
        if window.focused(cx).is_none() {
            self.focus.focus(window, cx);
        }
        let want_bg = if t.glass {
            gpui::WindowBackgroundAppearance::Blurred
        } else {
            gpui::WindowBackgroundAppearance::Opaque
        };
        if self.bg_appearance != want_bg {
            self.bg_appearance = want_bg;
            window.set_background_appearance(want_bg);
        }
        let vs = window.viewport_size();
        self.window_size = (f32::from(vs.width), f32::from(vs.height));
        if self.pending_default_tab {
            self.pending_default_tab = false;
            self.ensure_wt(Some(window), cx);
        }
        let (side_w, right_w, term_h) = self.sizes();
        let mut root = div()
            .id("workspace")
            .key_context("Workspace")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.chrome_ground)
            .text_color(t.ink)
            .font_family(metrics::UI_FONT)
            .text_size(metrics::TEXT)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.drag.take().is_some() {
                        this.save_prefs();
                        cx.notify();
                    }
                }),
            )
            .on_action(cx.listener(|this, _: &NewAgent, w, cx| {
                this.add_agent(Self::default_agent_index(), false, w, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, w, cx| this.open_settings(w, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, _, cx| {
                if let Some(id) = this
                    .wt()
                    .and_then(|ws| ws.tabs.get(ws.active))
                    .map(|t| t.id)
                {
                    this.close_tab(id, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.prefs.show_side = !this.prefs.show_side;
                this.save_prefs();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleRight, _, cx| {
                this.prefs.show_right = !this.prefs.show_right;
                this.save_prefs();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleTerminals, _, cx| {
                this.prefs.show_term = !this.prefs.show_term;
                this.save_prefs();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleTheme, _, cx| this.toggle_theme(cx)))
            .on_action(cx.listener(|this, _: &NewTerminal, _, cx| this.add_pane(None, cx)))
            .on_action(cx.listener(|this, _: &OpenProject, w, cx| this.add_project(w, cx)))
            .on_action(cx.listener(|this, _: &SaveFile, _, cx| this.save_file(cx)));
        if self.drag.is_some() {
            root = root.cursor(if matches!(self.drag, Some(Drag::Term { .. })) {
                gpui::CursorStyle::ResizeRow
            } else {
                gpui::CursorStyle::ResizeColumn
            });
        }

        root = root.child(self.render_top(&t, window, cx));
        if self.projects.is_empty() {
            return root
                .child(self.render_welcome(&t, cx))
                .child(self.render_status(&t, cx));
        }
        let middle = div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .when(side_w > 0., |d| {
                d.child(
                    div()
                        .w(px(side_w))
                        .flex_none()
                        .h_full()
                        .child(self.render_side(&t, window, cx)),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .when(t.glass, |d| d.p(px(GLASS_GAP)))
                    .child(self.render_center(&t, window, cx)),
            )
            .when(right_w > 0., |d| {
                d.child(
                    div()
                        .w(px(right_w))
                        .flex_none()
                        .h_full()
                        .child(self.render_right(&t, window, cx)),
                )
            })
            .when(side_w > 0., |d| {
                d.child(self.handle_v("side", side_w - 3., None, &t, cx))
            })
            .when(right_w > 0., |d| {
                d.child(self.handle_v("right", 0., Some(right_w - 2.), &t, cx))
            });
        root = root.child(middle);
        if term_h > 0. {
            root = root.child(
                div()
                    .h(px(term_h))
                    .flex_none()
                    .when(t.glass, |d| d.px(px(GLASS_GAP)).pb(px(GLASS_GAP)))
                    .child(self.render_bottom(&t, window, cx)),
            );
        }
        root = root.child(self.render_status(&t, cx));
        root = self.render_overlays(root, &t, window, cx);
        root
    }
}
