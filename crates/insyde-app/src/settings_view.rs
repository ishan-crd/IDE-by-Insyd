//! Settings screen: category pills across the top, a two-column grid of
//! setting tiles (control under the description, "Reset" when changed),
//! search across every category, and a "Changed only" filter. Every change
//! is saved to `settings.json` immediately and applied live.

use crate::ui::{self, icon};
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Entity, EventEmitter, FontWeight, SharedString, Subscription, Window, div,
    px,
};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use insyde_core::agents::AGENTS;
use insyde_core::settings::{self, Accent, Approval, OpenAs, Settings, ThemeChoice};
use insyde_theme::{ActiveTheme, Theme, metrics};
use std::collections::HashMap;

pub enum SettingsEvent {
    Close,
    /// Brains were deleted from disk; drop them from memory too.
    BrainsCleared,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cat {
    Look,
    Agents,
    Brain,
    Worktrees,
    Terminal,
    Editor,
    Alerts,
    Web,
    Data,
    Keys,
    About,
}

impl Cat {
    const ALL: [Cat; 11] = [
        Cat::Look,
        Cat::Agents,
        Cat::Brain,
        Cat::Worktrees,
        Cat::Terminal,
        Cat::Editor,
        Cat::Alerts,
        Cat::Web,
        Cat::Data,
        Cat::Keys,
        Cat::About,
    ];
    fn label(self) -> &'static str {
        match self {
            Cat::Look => "Look & feel",
            Cat::Agents => "Agents",
            Cat::Brain => "Project Brain",
            Cat::Worktrees => "Worktrees & Git",
            Cat::Terminal => "Terminal",
            Cat::Editor => "Editor",
            Cat::Alerts => "Notifications",
            Cat::Web => "Web access",
            Cat::Data => "Privacy & data",
            Cat::Keys => "Shortcuts",
            Cat::About => "About",
        }
    }
    fn blurb(self) -> &'static str {
        match self {
            Cat::Look => "Theme, accent color, glass, motion and reading size.",
            Cat::Agents => "Which agent starts, how it's approved, and how it's launched.",
            Cat::Brain => "How much project context every agent receives.",
            Cat::Worktrees => "Where task worktrees live and what happens when one is created.",
            Cat::Terminal => "Font, scrollback, shell and keyboard behavior.",
            Cat::Editor => "The file editor in the right panel.",
            Cat::Alerts => "When InsyDE should tap you on the shoulder.",
            Cat::Web => {
                "Use this Mac's agents, changes and terminals from a browser on your phone or any computer."
            }
            Cat::Data => "What is stored on this Mac, and how to clear it.",
            Cat::Keys => "Keyboard shortcuts.",
            Cat::About => "Version, files and links.",
        }
    }
}

/// Text settings edited through an input, keyed by id.
const TEXT_KEYS: &[&str] = &[
    "worktree_root",
    "branch_prefix",
    "setup_command",
    "copy_into_worktrees",
    "term_font",
    "shell",
    "web_port",
];

fn text_value(s: &Settings, key: &str) -> String {
    match key {
        "worktree_root" => s.worktree_root.clone(),
        "branch_prefix" => s.branch_prefix.clone(),
        "setup_command" => s.setup_command.clone(),
        "copy_into_worktrees" => s.copy_into_worktrees.clone(),
        "term_font" => s.term_font.clone(),
        "shell" => s.shell.clone(),
        "web_port" => s.web_port.to_string(),
        k => k
            .strip_prefix("cmd:")
            .and_then(|a| s.agent_commands.get(a).cloned())
            .unwrap_or_default(),
    }
}

fn set_text(s: &mut Settings, key: &str, v: String) {
    match key {
        "worktree_root" => s.worktree_root = v,
        "branch_prefix" => s.branch_prefix = v,
        "setup_command" => s.setup_command = v,
        "copy_into_worktrees" => s.copy_into_worktrees = v,
        "term_font" => s.term_font = v,
        "shell" => s.shell = v,
        "web_port" => {
            if let Ok(p) = v.trim().parse::<u16>()
                && p >= 1024
            {
                s.web_port = p;
            }
        }
        k => {
            if let Some(agent) = k.strip_prefix("cmd:") {
                if v.trim().is_empty() {
                    s.agent_commands.remove(agent);
                } else {
                    s.agent_commands.insert(agent.to_string(), v);
                }
            }
        }
    }
}

pub struct SettingsView {
    cat: Cat,
    search: Entity<InputState>,
    only_changed: bool,
    inputs: HashMap<String, Entity<InputState>>,
    env: Entity<TextareaState>,
    note: Option<String>,
    store: insyde_core::store::Store,
    _subs: Vec<Subscription>,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

impl SettingsView {
    pub fn new(
        store: insyde_core::store::Store,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let s = settings::get();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search every setting"));
        let mut subs = vec![cx.subscribe(&search, |_, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        })];
        let mut inputs = HashMap::new();
        let mut keys: Vec<String> = TEXT_KEYS.iter().map(|k| k.to_string()).collect();
        keys.extend(
            AGENTS
                .iter()
                .filter(|a| a.acp.is_some() && a.key != "super")
                .map(|a| format!("cmd:{}", a.key)),
        );
        for key in keys {
            let value = text_value(&s, &key);
            let placeholder = match key.as_str() {
                "worktree_root" => "../.insyde-worktrees/{repo}  (default)".to_string(),
                "setup_command" => "e.g. pnpm install".to_string(),
                "shell" => "$SHELL (default)".to_string(),
                "web_port" => "7788".to_string(),
                k => k
                    .strip_prefix("cmd:")
                    .and_then(|a| AGENTS.iter().find(|x| x.key == a))
                    .and_then(|a| a.acp)
                    .map(|c| format!("{} {}", c.program, c.args.join(" ")))
                    .unwrap_or_default(),
            };
            let input = cx.new(|cx| {
                let mut st = InputState::new(window, cx).placeholder(placeholder);
                st.set_value(value, window, cx);
                st
            });
            let k2 = key.clone();
            subs.push(cx.subscribe(&input, move |this, st, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    let v = st.read(cx).value().to_string();
                    let k = k2.clone();
                    settings::update(|s| set_text(s, &k, v));
                    this.changed(cx);
                }
            }));
            inputs.insert(key, input);
        }
        let env = cx.new(|cx| {
            let mut st = TextareaState::new(window, cx)
                .auto_grow(3, 8)
                .placeholder("KEY=value, one per line");
            st.set_value(s.agent_env.clone(), window, cx);
            st
        });
        subs.push(cx.subscribe(&env, |this, st, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                let v = st.read(cx).value().to_string();
                settings::update(|s| s.agent_env = v);
                this.changed(cx);
            }
        }));
        search.update(cx, |st, cx| st.focus(window, cx));
        // Keep the Web access page live (tunnel link, connected browsers).
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let alive = this.update(cx, |this, cx| {
                    if this.cat == Cat::Web {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            cat: Cat::Look,
            search,
            only_changed: false,
            inputs,
            env,
            note: None,
            store,
            _subs: subs,
        }
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        // Starts, restarts or stops the web server if its settings moved.
        crate::web_access::sync(&self.store);
        cx.refresh_windows();
        cx.notify();
    }

    fn set(&mut self, f: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        let before = settings::get();
        let after = settings::update(f);
        if before.theme != after.theme
            || before.accent != after.accent
            || before.glass != after.glass
            || before.glass_tint != after.glass_tint
        {
            crate::prefs::apply_theme(cx);
        }
        self.changed(cx);
    }

    /// Put every text input back in sync with the settings (after a reset).
    fn resync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = settings::get();
        for (key, input) in &self.inputs {
            let v = text_value(&s, key);
            if input.read(cx).value().as_ref() != v {
                input.update(cx, |st, cx| st.set_value(v, window, cx));
            }
        }
        if self.env.read(cx).value().as_ref() != s.agent_env {
            let v = s.agent_env.clone();
            self.env.update(cx, |st, cx| st.set_value(v, window, cx));
        }
    }
}

/// One setting, as data. `render` builds its control.
struct Tile {
    cat: Cat,
    title: &'static str,
    desc: &'static str,
    changed: bool,
    reset: Option<fn(&mut Settings)>,
    control: AnyElement,
}

impl SettingsView {
    fn toggle(
        &self,
        id: &'static str,
        on: bool,
        set: fn(&mut Settings, bool),
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .child(ui::switch(on, t))
            .child(
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(if on { t.ink } else { t.ink_3 })
                    .child(if on { "On" } else { "Off" }),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set(|s| set(s, !on), cx)))
            .into_any_element()
    }

    fn choice<T: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        opts: &[(&'static str, T)],
        cur: T,
        set: fn(&mut Settings, T),
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut row = div().flex().flex_wrap().gap(px(6.));
        for (i, (label, v)) in opts.iter().copied().enumerate() {
            let on = v == cur;
            let hover = t.hover;
            row = row.child(
                div()
                    .id(SharedString::from(format!("{id}-{i}")))
                    .h(px(28.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .rounded(px(14.))
                    .cursor_pointer()
                    .text_size(metrics::TEXT_SM)
                    .font_weight(FontWeight::MEDIUM)
                    .border_1()
                    .when(on, |d| {
                        d.bg(t.sel_bg).border_color(t.accent).text_color(t.sel_text)
                    })
                    .when(!on, |d| {
                        d.border_color(t.field_border)
                            .text_color(t.ink_2)
                            .hover(move |s| s.bg(hover))
                    })
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| this.set(|s| set(s, v), cx))),
            );
        }
        row.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn stepper(
        &self,
        id: &'static str,
        value: f32,
        step: f32,
        min: f32,
        max: f32,
        unit: &'static str,
        set: fn(&mut Settings, f32),
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fmt = if step.fract() == 0. {
            format!("{}{unit}", value as i64)
        } else {
            format!("{value:.1}{unit}")
        };
        let btn = |sid: String, glyph: &'static str, v: f32, cx: &mut Context<Self>| {
            ui::icon_button(SharedString::from(sid), glyph, 11., t)
                .size(px(26.))
                .border_1()
                .border_color(t.field_border)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let nv = (v * 100.).round() / 100.;
                    this.set(|s| set(s, nv.clamp(min, max)), cx)
                }))
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(btn(format!("{id}-dec"), "minus", value - step, cx))
            .child(
                div()
                    .min_w(px(64.))
                    .text_center()
                    .font_family(metrics::MONO_FONT)
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink)
                    .child(fmt),
            )
            .child(btn(format!("{id}-inc"), "plus", value + step, cx))
            .into_any_element()
    }

    fn text(&self, key: &str) -> AnyElement {
        match self.inputs.get(key) {
            Some(i) => Input::new(i).h(px(30.)).into_any_element(),
            None => div().into_any_element(),
        }
    }

    fn tiles(&mut self, t: &Theme, cx: &mut Context<Self>) -> Vec<Tile> {
        let s = settings::get();
        let d = Settings::default();
        let mut v = Vec::new();
        macro_rules! tile {
            ($cat:expr, $title:expr, $desc:expr, $changed:expr, $reset:expr, $control:expr) => {
                v.push(Tile {
                    cat: $cat,
                    title: $title,
                    desc: $desc,
                    changed: $changed,
                    reset: $reset,
                    control: $control,
                })
            };
        }

        // Look & feel
        tile!(
            Cat::Look,
            "Theme",
            "Follow macOS, or pin light or dark.",
            s.theme != d.theme,
            Some(|s: &mut Settings| s.theme = Settings::default().theme),
            self.choice(
                "theme",
                &[
                    ("Dark", ThemeChoice::Dark),
                    ("Light", ThemeChoice::Light),
                    ("Match macOS", ThemeChoice::System)
                ],
                s.theme,
                |s, v| s.theme = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Look,
            "Accent color",
            "Buttons, focus rings, selection and links.",
            s.accent != d.accent,
            Some(|s: &mut Settings| s.accent = Settings::default().accent),
            self.accent_picker(s.accent, t, cx)
        );
        tile!(
            Cat::Look,
            "Chat text size",
            "Size of agent conversations.",
            s.chat_text_size != d.chat_text_size,
            Some(|s: &mut Settings| s.chat_text_size = Settings::default().chat_text_size),
            self.stepper(
                "chat-size",
                s.chat_text_size,
                1.,
                11.,
                18.,
                "px",
                |s, v| s.chat_text_size = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Look,
            "Glass",
            "See your desktop, softly blurred, through the sidebars and top bar. Chats and terminals float on top as solid sheets.",
            s.glass != d.glass,
            Some(|s: &mut Settings| s.glass = Settings::default().glass),
            self.toggle("glass", s.glass, |s, v| s.glass = v, t, cx)
        );
        tile!(
            Cat::Look,
            "Glass tint",
            "How much color covers the blur. Lower is clearer, higher is easier to read.",
            s.glass_tint != d.glass_tint,
            Some(|s: &mut Settings| s.glass_tint = Settings::default().glass_tint),
            self.stepper(
                "glass-tint",
                s.glass_tint as f32,
                10.,
                30.,
                90.,
                "%",
                |s, v| s.glass_tint = v as u32,
                t,
                cx
            )
        );
        tile!(
            Cat::Look,
            "Reduce motion",
            "Stop pulsing activity dots and other animation.",
            s.reduce_motion != d.reduce_motion,
            Some(|s: &mut Settings| s.reduce_motion = false),
            self.toggle("motion", s.reduce_motion, |s, v| s.reduce_motion = v, t, cx)
        );

        // Agents
        let agent_opts: Vec<(&'static str, &'static str)> = AGENTS
            .iter()
            .filter(|a| a.acp.is_some() && a.key != "super")
            .map(|a| (a.name, a.key))
            .collect();
        let cur_agent: &'static str = agent_opts
            .iter()
            .find(|(_, k)| *k == s.default_agent)
            .map(|(_, k)| *k)
            .unwrap_or("claude");
        tile!(
            Cat::Agents,
            "Default agent",
            "Used by + Agent, ⌘T and new worktrees.",
            s.default_agent != d.default_agent,
            Some(|s: &mut Settings| s.default_agent = Settings::default().default_agent),
            self.choice(
                "default-agent",
                &agent_opts,
                cur_agent,
                |s, v| s.default_agent = v.to_string(),
                t,
                cx
            )
        );
        tile!(
            Cat::Agents,
            "Open new agents in",
            "What + Agent opens: InsyDE's chat window, or the agent's own app in a terminal tab. Also switchable at the bottom of the + Agent menu; holding ⌘ picks the other one.",
            s.open_agents_as != d.open_agents_as,
            Some(|s: &mut Settings| s.open_agents_as = OpenAs::Chat),
            self.choice(
                "open-as",
                &[
                    ("Chat window", OpenAs::Chat),
                    ("Terminal", OpenAs::Terminal)
                ],
                s.open_agents_as,
                |s, v| s.open_agents_as = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Agents,
            "Permissions by default",
            "Ask before every action, auto-approve file edits, or skip permission prompts entirely. In terminals this passes the agent's own flag (Claude Code --dangerously-skip-permissions, Codex --dangerously-bypass-approvals-and-sandbox, Grok bypassPermissions). Each chat can still change it.",
            s.approval != d.approval,
            Some(|s: &mut Settings| s.approval = Settings::default().approval),
            self.choice(
                "approval",
                &[
                    ("Ask for permission", Approval::Ask),
                    ("Auto-approve edits", Approval::AcceptEdits),
                    ("Skip permissions", Approval::FullAccess)
                ],
                s.approval,
                |s, v| s.approval = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Agents,
            "Start agents immediately",
            "Launch the agent when its tab opens instead of on your first message (faster first reply, more memory).",
            s.start_agents_eagerly != d.start_agents_eagerly,
            Some(|s: &mut Settings| s.start_agents_eagerly = false),
            self.toggle(
                "eager",
                s.start_agents_eagerly,
                |s, v| s.start_agents_eagerly = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Agents,
            "Let agents coordinate",
            "Auto-approve shell commands made only of `insy …` calls (teams, hand-offs). Nothing else is affected.",
            s.auto_approve_insy != d.auto_approve_insy,
            Some(|s: &mut Settings| s.auto_approve_insy = true),
            self.toggle(
                "insy",
                s.auto_approve_insy,
                |s, v| s.auto_approve_insy = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Agents,
            "Context warning",
            "Suggest a hand-off when an agent's context window is this full.",
            s.context_warning_pct != d.context_warning_pct,
            Some(|s: &mut Settings| s.context_warning_pct = 85),
            self.stepper(
                "ctx-warn",
                s.context_warning_pct as f32,
                5.,
                50.,
                99.,
                "%",
                |s, v| s.context_warning_pct = v as u32,
                t,
                cx
            )
        );
        let mut cmds = div().flex().flex_col().gap(px(6.));
        for (name, key) in &agent_opts {
            cmds = cmds.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .w(px(92.))
                            .flex_none()
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child(*name),
                    )
                    .child(div().flex_1().child(self.text(&format!("cmd:{key}")))),
            );
        }
        tile!(
            Cat::Agents,
            "Launch commands",
            "Override how an agent's ACP adapter starts (pin a version, use a local build). Empty uses the default shown.",
            !s.agent_commands.is_empty(),
            Some(|s: &mut Settings| s.agent_commands.clear()),
            cmds.into_any_element()
        );
        tile!(
            Cat::Agents,
            "Environment",
            "Extra variables for agents and terminals, e.g. API endpoints or proxies. Lines starting with # are ignored.",
            s.agent_env != d.agent_env,
            Some(|s: &mut Settings| s.agent_env.clear()),
            div()
                .bg(t.panel_2)
                .border_1()
                .border_color(t.field_border)
                .rounded(px(6.))
                .p(px(4.))
                .child(Textarea::new(&self.env).appearance(false))
                .into_any_element()
        );

        // Brain
        tile!(
            Cat::Brain,
            "Include the brain by default",
            "New chats start with the Project Brain digest. You can switch it off per chat.",
            s.brain_by_default != d.brain_by_default,
            Some(|s: &mut Settings| s.brain_by_default = true),
            self.toggle(
                "brain-default",
                s.brain_by_default,
                |s, v| s.brain_by_default = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Brain,
            "Context budget",
            "Tokens of brain context sent with a first message. Pinned decisions and conventions always come first.",
            s.brain_budget_tokens != d.brain_budget_tokens,
            Some(|s: &mut Settings| s.brain_budget_tokens = 18_000),
            self.stepper(
                "budget",
                s.brain_budget_tokens as f32 / 1000.,
                2.,
                2.,
                100.,
                "k",
                |s, v| s.brain_budget_tokens = (v * 1000.) as u32,
                t,
                cx
            )
        );
        tile!(
            Cat::Brain,
            "Keep the brain up to date",
            "Rebuild automatically when the base branch gets new commits (checked on each refresh).",
            s.brain_auto_update != d.brain_auto_update,
            Some(|s: &mut Settings| s.brain_auto_update = false),
            self.toggle(
                "brain-auto",
                s.brain_auto_update,
                |s, v| s.brain_auto_update = v,
                t,
                cx
            )
        );

        // Worktrees & Git
        tile!(
            Cat::Worktrees,
            "Worktree location",
            "Folder for task worktrees. {repo} is the repository name; relative paths are inside the repo.",
            s.worktree_root != d.worktree_root,
            Some(|s: &mut Settings| s.worktree_root.clear()),
            self.text("worktree_root")
        );
        tile!(
            Cat::Worktrees,
            "Branch prefix",
            "Used when a task title has no prefix (fix/…, chore/…).",
            s.branch_prefix != d.branch_prefix,
            Some(|s: &mut Settings| s.branch_prefix = "feat".into()),
            self.text("branch_prefix")
        );
        tile!(
            Cat::Worktrees,
            "Setup command",
            "Runs in the first terminal of every new worktree.",
            s.setup_command != d.setup_command,
            Some(|s: &mut Settings| s.setup_command.clear()),
            self.text("setup_command")
        );
        tile!(
            Cat::Worktrees,
            "Copy into new worktrees",
            "Untracked files from the main checkout, comma-separated (secrets, local config).",
            s.copy_into_worktrees != d.copy_into_worktrees,
            Some(|s: &mut Settings| s.copy_into_worktrees = Settings::default().copy_into_worktrees),
            self.text("copy_into_worktrees")
        );
        tile!(
            Cat::Worktrees,
            "Refresh every",
            "How often worktrees, diffs and PR checks are re-read.",
            s.refresh_secs != d.refresh_secs,
            Some(|s: &mut Settings| s.refresh_secs = 20),
            self.stepper(
                "refresh",
                s.refresh_secs as f32,
                5.,
                5.,
                600.,
                "s",
                |s, v| s.refresh_secs = v as u32,
                t,
                cx
            )
        );
        tile!(
            Cat::Worktrees,
            "Open PRs as drafts",
            "Create PR opens a draft you can mark ready on GitHub.",
            s.pr_draft != d.pr_draft,
            Some(|s: &mut Settings| s.pr_draft = true),
            self.toggle("draft", s.pr_draft, |s, v| s.pr_draft = v, t, cx)
        );
        tile!(
            Cat::Worktrees,
            "Confirm before removing",
            "Removing a worktree takes a second click on the bin.",
            s.confirm_worktree_delete != d.confirm_worktree_delete,
            Some(|s: &mut Settings| s.confirm_worktree_delete = true),
            self.toggle(
                "confirm",
                s.confirm_worktree_delete,
                |s, v| s.confirm_worktree_delete = v,
                t,
                cx
            )
        );

        // Terminal
        tile!(
            Cat::Terminal,
            "Font",
            "Any installed monospaced font.",
            s.term_font != d.term_font,
            Some(|s: &mut Settings| s.term_font = "Menlo".into()),
            self.text("term_font")
        );
        tile!(
            Cat::Terminal,
            "Font size",
            "⌘+ and ⌘− also resize a single terminal.",
            s.term_font_size != d.term_font_size,
            Some(|s: &mut Settings| s.term_font_size = 11.5),
            self.stepper(
                "tsize",
                s.term_font_size,
                0.5,
                8.,
                28.,
                "px",
                |s, v| s.term_font_size = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Terminal,
            "Line height",
            "Spacing between rows.",
            s.term_line_height != d.term_line_height,
            Some(|s: &mut Settings| s.term_line_height = 1.7),
            self.stepper(
                "lh",
                s.term_line_height,
                0.1,
                1.,
                2.5,
                "×",
                |s, v| s.term_line_height = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Terminal,
            "Scrollback",
            "Lines kept per terminal (new terminals).",
            s.term_scrollback != d.term_scrollback,
            Some(|s: &mut Settings| s.term_scrollback = 10_000),
            self.stepper(
                "scroll",
                s.term_scrollback as f32 / 1000.,
                5.,
                1.,
                200.,
                "k lines",
                |s, v| s.term_scrollback = (v * 1000.) as u32,
                t,
                cx
            )
        );
        tile!(
            Cat::Terminal,
            "Shell",
            "Program for new terminals. Empty uses your login shell.",
            s.shell != d.shell,
            Some(|s: &mut Settings| s.shell.clear()),
            self.text("shell")
        );
        tile!(
            Cat::Terminal,
            "Copy on select",
            "Selecting text copies it immediately.",
            s.copy_on_select != d.copy_on_select,
            Some(|s: &mut Settings| s.copy_on_select = false),
            self.toggle("cos", s.copy_on_select, |s, v| s.copy_on_select = v, t, cx)
        );
        tile!(
            Cat::Terminal,
            "Option key acts as Meta",
            "⌥B / ⌥F jump by word in shells. Off types macOS characters like ∫.",
            s.option_as_meta != d.option_as_meta,
            Some(|s: &mut Settings| s.option_as_meta = true),
            self.toggle("meta", s.option_as_meta, |s, v| s.option_as_meta = v, t, cx)
        );

        // Editor
        tile!(
            Cat::Editor,
            "Wrap long lines",
            "Soft-wrap instead of scrolling sideways.",
            s.editor_soft_wrap != d.editor_soft_wrap,
            Some(|s: &mut Settings| s.editor_soft_wrap = false),
            self.toggle(
                "wrap",
                s.editor_soft_wrap,
                |s, v| s.editor_soft_wrap = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Editor,
            "Line numbers",
            "Show the gutter.",
            s.editor_line_numbers != d.editor_line_numbers,
            Some(|s: &mut Settings| s.editor_line_numbers = true),
            self.toggle(
                "lines",
                s.editor_line_numbers,
                |s, v| s.editor_line_numbers = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Editor,
            "Tab width",
            "Spaces per indent level.",
            s.editor_tab_size != d.editor_tab_size,
            Some(|s: &mut Settings| s.editor_tab_size = 4),
            self.choice(
                "tabs",
                &[("2", 2u32), ("4", 4), ("8", 8)],
                s.editor_tab_size,
                |s, v| s.editor_tab_size = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Editor,
            "Save when switching files",
            "Opening another file saves the current one instead of asking.",
            s.editor_autosave != d.editor_autosave,
            Some(|s: &mut Settings| s.editor_autosave = false),
            self.toggle(
                "autosave",
                s.editor_autosave,
                |s, v| s.editor_autosave = v,
                t,
                cx
            )
        );

        // Notifications
        tile!(
            Cat::Alerts,
            "When an agent finishes",
            "A macOS notification with the first line of its reply.",
            s.notify_turn_done != d.notify_turn_done,
            Some(|s: &mut Settings| s.notify_turn_done = true),
            self.toggle(
                "n-done",
                s.notify_turn_done,
                |s, v| s.notify_turn_done = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Alerts,
            "When an agent needs approval",
            "So a waiting agent never sits idle.",
            s.notify_permission != d.notify_permission,
            Some(|s: &mut Settings| s.notify_permission = true),
            self.toggle(
                "n-perm",
                s.notify_permission,
                |s, v| s.notify_permission = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Alerts,
            "Only when InsyDE is in the background",
            "Stay quiet while you're looking at the app.",
            s.notify_only_background != d.notify_only_background,
            Some(|s: &mut Settings| s.notify_only_background = true),
            self.toggle(
                "n-bg",
                s.notify_only_background,
                |s, v| s.notify_only_background = v,
                t,
                cx
            )
        );

        // Privacy & data
        // Web access
        tile!(
            Cat::Web,
            "Web access",
            "Serve the InsyDE web client from this Mac while the app is open. Browsers pair once with a private link.",
            s.web_access != d.web_access,
            Some(|s: &mut Settings| s.web_access = Settings::default().web_access),
            self.toggle("web-access", s.web_access, |s, v| s.web_access = v, t, cx)
        );
        tile!(
            Cat::Web,
            "Who can connect",
            "This Mac only, or any device on your network and your Tailscale tailnet.",
            s.web_network != d.web_network,
            Some(|s: &mut Settings| s.web_network = Settings::default().web_network),
            self.choice(
                "web-net",
                &[("This Mac only", false), ("Network & Tailscale", true)],
                s.web_network,
                |s, v| s.web_network = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Web,
            "Public link",
            "A temporary https address through Cloudflare, for when you are away from your network. Needs cloudflared.",
            s.web_tunnel != d.web_tunnel,
            Some(|s: &mut Settings| s.web_tunnel = Settings::default().web_tunnel),
            self.toggle("web-tunnel", s.web_tunnel, |s, v| s.web_tunnel = v, t, cx)
        );
        tile!(
            Cat::Web,
            "Port",
            "The local port the web client listens on.",
            s.web_port != d.web_port,
            Some(|s: &mut Settings| s.web_port = Settings::default().web_port),
            self.text("web_port")
        );

        tile!(
            Cat::Data,
            "Keep conversation history",
            "Store transcripts so sessions come back after a restart. Off keeps new turns in memory only.",
            s.store_transcripts != d.store_transcripts,
            Some(|s: &mut Settings| s.store_transcripts = true),
            self.toggle(
                "history",
                s.store_transcripts,
                |s, v| s.store_transcripts = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Data,
            "Local control socket",
            "Lets `insy` and agents drive InsyDE. Only your user can connect. Takes effect after restart.",
            s.control_socket != d.control_socket,
            Some(|s: &mut Settings| s.control_socket = true),
            self.toggle(
                "socket",
                s.control_socket,
                |s, v| s.control_socket = v,
                t,
                cx
            )
        );
        tile!(
            Cat::Data,
            "Clean up",
            "Nothing leaves your Mac; these only delete local data.",
            false,
            None,
            self.cleanup(t, cx)
        );

        v
    }

    fn accent_picker(&self, cur: Accent, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut row = div().flex().gap(px(10.));
        for (i, (a, hex)) in [
            (Accent::Blue, 0x2F6BEB),
            (Accent::Violet, 0x7C5CD6),
            (Accent::Green, 0x2E9E6B),
            (Accent::Orange, 0xE0733A),
            (Accent::Pink, 0xD6457F),
        ]
        .into_iter()
        .enumerate()
        {
            let on = a == cur;
            row = row.child(
                div()
                    .id(("accent", i))
                    .size(px(26.))
                    .rounded_full()
                    .p(px(3.))
                    .border_2()
                    .border_color(if on { t.ink } else { gpui::transparent_black() })
                    .cursor_pointer()
                    .child(
                        div()
                            .size_full()
                            .rounded_full()
                            .bg(insyde_theme::color(hex)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.set(|s| s.accent = a, cx))),
            );
        }
        row.into_any_element()
    }

    fn cleanup(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .child(
                ui::small_button("clear-history", "Delete closed sessions", t).on_click(
                    cx.listener(|this, _, _, cx| {
                        let n = this.store.clear_closed_sessions();
                        this.note = Some(format!("Deleted {n} closed sessions."));
                        cx.notify();
                    }),
                ),
            )
            .child(
                ui::small_button("clear-brains", "Delete all brains", t).on_click(cx.listener(
                    |this, _, _, cx| {
                        let _ =
                            std::fs::remove_dir_all(insyde_core::store::data_dir().join("brains"));
                        this.note = Some(
                            "Deleted every Project Brain. Create one again from the top bar."
                                .into(),
                        );
                        cx.emit(SettingsEvent::BrainsCleared);
                        cx.notify();
                    },
                )),
            )
            .child(
                ui::small_button("open-data", "Show data folder", t).on_click(|_, _, cx| {
                    cx.open_url(&format!(
                        "file://{}",
                        insyde_core::store::data_dir().display()
                    ));
                }),
            )
            .into_any_element()
    }

    fn render_tile(&self, tile: Tile, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let reset = tile.reset.filter(|_| tile.changed);
        let title = tile.title;
        div()
            .w(px(506.))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(14.))
            .bg(t.panel)
            .border_1()
            .border_color(if tile.changed { t.sel_chip } else { t.line })
            .rounded(metrics::RADIUS_LG)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(metrics::TEXT)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.ink)
                            .child(title),
                    )
                    .when_some(reset, |d, r| {
                        d.child(
                            div()
                                .id(SharedString::from(format!("reset-{title}")))
                                .text_size(metrics::TEXT_XS)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.accent)
                                .cursor_pointer()
                                .child("Reset")
                                .on_click(cx.listener(move |this, _, w, cx| {
                                    this.set(r, cx);
                                    this.resync_inputs(w, cx);
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_3)
                    .child(tile.desc),
            )
            .child(tile.control)
            .into_any_element()
    }

    fn render_keys(&self, t: &Theme) -> AnyElement {
        let rows: &[(&str, &str)] = &[
            ("⌘T", "New agent (default agent)"),
            ("⌘W", "Close tab"),
            ("⌘,", "Settings"),
            ("⌘O", "Open a repository"),
            ("⌘B", "Toggle sidebar"),
            ("⌥⌘B", "Toggle right panel"),
            ("⌘J", "Toggle terminals"),
            ("⇧⌘L", "Toggle light / dark"),
            ("⌃`", "New terminal pane"),
            ("⌘S", "Save file"),
            ("1–9", "Pick an agent (agent menu open)"),
            ("Enter · ⇧Enter", "Send · new line (composer)"),
            ("⌘C · ⌘V · ⌘K", "Copy selection · paste · clear (terminal)"),
            ("⌘+ · ⌘−", "Terminal font size"),
        ];
        let mut col = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .w(px(1024.))
            .p(px(10.))
            .bg(t.panel)
            .border_1()
            .border_color(t.line)
            .rounded(metrics::RADIUS_LG);
        for (k, what) in rows {
            col = col.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .h(px(32.))
                    .px(px(8.))
                    .child(
                        div().w(px(160.)).flex_none().child(
                            div()
                                .px(px(8.))
                                .py(px(2.))
                                .rounded(px(5.))
                                .bg(t.hover_2)
                                .border_1()
                                .border_color(t.field_border)
                                .font_family(metrics::MONO_FONT)
                                .text_size(metrics::TEXT_SM)
                                .text_color(t.ink)
                                .child(*k),
                        ),
                    )
                    .child(
                        div()
                            .text_size(metrics::TEXT_SM)
                            .text_color(t.ink_2)
                            .child(*what),
                    ),
            );
        }
        col.into_any_element()
    }

    /// Pairing panel on the Web access page: QR code, links, connected browsers.
    fn render_web(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let panel = div()
            .w(px(1024.))
            .p(px(16.))
            .flex()
            .gap(px(20.))
            .bg(t.panel)
            .border_1()
            .border_color(t.line)
            .rounded(metrics::RADIUS_LG);
        let Some(snap) = crate::web_access::snapshot() else {
            return panel
                .items_center()
                .child(ui::icon("popout", 16., t.ink_3))
                .child(
                    div()
                        .flex_1()
                        .text_size(metrics::TEXT_SM)
                        .text_color(t.ink_3)
                        .child("Turn on Web access to open InsyDE from a browser. Your agents, changes and terminals keep running here; the browser is a window onto them."),
                )
                .into_any_element();
        };
        if let Some(e) = snap.error {
            return panel
                .items_center()
                .child(ui::icon("cross", 14., t.err))
                .child(
                    div()
                        .flex_1()
                        .text_size(metrics::TEXT_SM)
                        .text_color(t.err)
                        .child(e),
                )
                .into_any_element();
        }
        // The QR code opens the most reachable link (public, then network, then local).
        let best = snap
            .links
            .iter()
            .rev()
            .find(|l| !l.local)
            .or(snap.links.first())
            .cloned();
        let qr = best.as_ref().and_then(|l| insyde_core::web::qr(&l.url));
        let mut left = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .w(px(200.))
            .flex_none();
        if let Some((w, dark)) = qr {
            let module = (184. / (w as f32 + 4.)).floor().max(2.);
            let mut code = div()
                .p(px(module * 2.))
                .bg(t.palette.white)
                .rounded(px(8.))
                .flex()
                .flex_col();
            for y in 0..w {
                // One element per run of dark modules keeps the element count low.
                let mut row = div().flex().h(px(module));
                let mut x = 0;
                while x < w {
                    let on = dark[y * w + x];
                    let start = x;
                    while x < w && dark[y * w + x] == on {
                        x += 1;
                    }
                    let run = div().w(px(module * (x - start) as f32)).h_full();
                    row = row.child(if on { run.bg(gpui::black()) } else { run });
                }
                code = code.child(row);
            }
            left = left.child(code);
        }
        left = left.child(
            div()
                .text_size(metrics::TEXT_XS)
                .text_color(t.ink_3)
                .text_center()
                .child(match &best {
                    Some(l) if !l.local => format!("Scan with your phone ({})", l.label),
                    _ => "Choose Network & Tailscale or Public link to pair a phone".into(),
                }),
        );

        let mut right = div().flex_1().min_w_0().flex().flex_col().gap(px(8.));
        right = right.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::dot(t.ok, 7.))
                .child(
                    div()
                        .text_size(metrics::TEXT)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.ink)
                        .child("Web access is on"),
                )
                .child(div().text_size(metrics::TEXT_SM).text_color(t.ink_3).child(
                    match snap.clients {
                        0 => "No browsers connected".to_string(),
                        1 => "1 browser connected".to_string(),
                        n => format!("{n} browsers connected"),
                    },
                )),
        );
        for (i, l) in snap.links.iter().enumerate() {
            let url = l.url.clone();
            let url2 = l.url.clone();
            // Show the address without the pairing token.
            let shown = l.url.split("/#").next().unwrap_or(&l.url).to_string();
            right = right.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(34.))
                    .px(px(10.))
                    .rounded(metrics::RADIUS)
                    .bg(t.panel_2)
                    .border_1()
                    .border_color(t.line_soft)
                    .child(
                        div()
                            .w(px(110.))
                            .text_size(metrics::TEXT_SM)
                            .text_color(t.ink_3)
                            .child(l.label),
                    )
                    .child(
                        ui::trunc(shown)
                            .flex_1()
                            .font_family(metrics::MONO_FONT)
                            .text_size(metrics::TEXT_SM)
                            .text_color(t.ink),
                    )
                    .child(
                        ui::small_button(
                            SharedString::from(format!("web-copy-{i}")),
                            "Copy link",
                            t,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(url.clone()));
                            this.note = Some("Pairing link copied".into());
                            cx.notify();
                        })),
                    )
                    .child(
                        ui::small_button(SharedString::from(format!("web-open-{i}")), "Open", t)
                            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url2))),
                    ),
            );
        }
        if snap.tunnel_pending {
            right = right.child(
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_3)
                    .child("Creating a public link…"),
            );
        }
        if let Some(e) = snap.tunnel_error {
            right = right.child(
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.warn)
                    .child(e),
            );
        }
        right = right.child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .pt(px(4.))
                .child(
                    ui::small_button("web-new-link", "New link", t).on_click(cx.listener(|this, _, _, cx| {
                        crate::web_access::new_link();
                        this.note = Some("New pairing link: other browsers were signed out".into());
                        cx.notify();
                    })),
                )
                .child(
                    div()
                        .flex_1()
                        .text_size(metrics::TEXT_XS)
                        .text_color(t.ink_3)
                        .child("A pairing link works like a key to this Mac. Share it only with your own devices; make a new one to sign every browser out."),
                ),
        );
        panel.child(left).child(right).into_any_element()
    }

    fn render_about(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let row = |label: &'static str, value: String| {
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .h(px(34.))
                .child(
                    div()
                        .w(px(160.))
                        .text_size(metrics::TEXT_SM)
                        .text_color(t.ink_3)
                        .child(label),
                )
                .child(
                    ui::trunc(value)
                        .flex_1()
                        .font_family(metrics::MONO_FONT)
                        .text_size(metrics::TEXT_SM)
                        .text_color(t.ink),
                )
        };
        div()
            .w(px(1024.))
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .bg(t.panel)
            .border_1()
            .border_color(t.line)
            .rounded(metrics::RADIUS_LG)
            .child(
                div()
                    .text_size(px(18.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.ink)
                    .child("InsyDE by Insyd"),
            )
            .child(
                div()
                    .pb(px(8.))
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_3)
                    .child("A native IDE for AI coding agents."),
            )
            .child(row("Version", env!("CARGO_PKG_VERSION").to_string()))
            .child(row("Settings file", settings::path().display().to_string()))
            .child(row(
                "Data folder",
                insyde_core::store::data_dir().display().to_string(),
            ))
            .child(row(
                "Control socket",
                insyde_core::rpc::socket_path().display().to_string(),
            ))
            .child(
                div()
                    .pt(px(10.))
                    .flex()
                    .gap(px(8.))
                    .child(
                        ui::small_button("gh", "Source on GitHub", t).on_click(|_, _, cx| {
                            cx.open_url("https://github.com/ishan-crd/IDE-by-Insyd")
                        }),
                    )
                    .child(
                        ui::small_button("releases", "Check for updates", t).on_click(
                            |_, _, cx| {
                                cx.open_url("https://github.com/ishan-crd/IDE-by-Insyd/releases")
                            },
                        ),
                    )
                    .child(
                        ui::small_button("reload", "Reload settings.json", t).on_click(
                            cx.listener(|this, _, w, cx| {
                                settings::reload();
                                crate::prefs::apply_theme(cx);
                                this.resync_inputs(w, cx);
                                this.note = Some("Reloaded settings.json".into());
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let query = self.search.read(cx).value().to_lowercase();
        let tiles = self.tiles(&t, cx);
        let changed_by_cat = |c: Cat| tiles.iter().filter(|x| x.cat == c && x.changed).count();
        let total_changed = tiles.iter().filter(|x| x.changed).count();

        // Category pills.
        let mut pills = div().flex().flex_wrap().gap(px(4.));
        for c in Cat::ALL {
            let on = c == self.cat && query.is_empty();
            let n = changed_by_cat(c);
            let hover = t.hover;
            pills = pills.child(
                div()
                    .id(SharedString::from(format!("cat-{c:?}")))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(30.))
                    .px(px(10.))
                    .rounded(px(15.))
                    .cursor_pointer()
                    .text_size(metrics::TEXT_SM)
                    .font_weight(FontWeight::MEDIUM)
                    .when(on, |d| d.bg(t.primary).text_color(t.on_primary))
                    .when(!on, |d| {
                        d.bg(t.panel)
                            .border_1()
                            .border_color(t.line)
                            .text_color(t.ink_2)
                            .hover(move |s| s.bg(hover))
                    })
                    .child(c.label())
                    .when(n > 0, |d| {
                        d.child(
                            div()
                                .min_w(px(16.))
                                .h(px(16.))
                                .px(px(4.))
                                .rounded(px(8.))
                                .bg(t.accent)
                                .text_color(t.palette.white)
                                .text_size(px(10.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(n.to_string()),
                        )
                    })
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.cat = c;
                        this.search.update(cx, |s, cx| s.set_value("", w, cx));
                        cx.notify();
                    })),
            );
        }

        // Body.
        let searching = !query.is_empty();
        let visible: Vec<Tile> = tiles
            .into_iter()
            .filter(|x| {
                if searching {
                    x.title.to_lowercase().contains(&query)
                        || x.desc.to_lowercase().contains(&query)
                } else {
                    x.cat == self.cat
                }
            })
            .filter(|x| !self.only_changed || x.changed)
            .collect();
        let mut body = div().flex().flex_col().gap(px(14.)).w(px(1024.));
        if !searching {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.ink)
                            .child(self.cat.label()),
                    )
                    .child(
                        div()
                            .text_size(metrics::TEXT_SM)
                            .text_color(t.ink_3)
                            .child(self.cat.blurb()),
                    ),
            );
        }
        if !searching && self.cat == Cat::Keys {
            body = body.child(self.render_keys(&t));
        } else if !searching && self.cat == Cat::About {
            body = body.child(self.render_about(&t, cx));
        } else if visible.is_empty() {
            body = body.child(
                div()
                    .py(px(40.))
                    .text_center()
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_3)
                    .child(if self.only_changed {
                        "Nothing changed here: everything is on its default."
                    } else {
                        "No settings match."
                    }),
            );
        } else {
            if !searching && self.cat == Cat::Web {
                body = body.child(self.render_web(&t, cx));
            }
            let mut last: Option<Cat> = None;
            let mut grid = div().flex().flex_wrap().gap(px(12.));
            for tile in visible {
                if searching && last != Some(tile.cat) {
                    if last.is_some() {
                        body = body.child(grid);
                        grid = div().flex().flex_wrap().gap(px(12.));
                    }
                    body = body.child(
                        div()
                            .pt(px(6.))
                            .text_size(metrics::TEXT_XS)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.ink_3)
                            .child(tile.cat.label().to_uppercase()),
                    );
                    last = Some(tile.cat);
                }
                grid = grid.child(self.render_tile(tile, &t, cx));
            }
            body = body.child(grid);
        }

        let header = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .h(metrics::TOPBAR_H)
            .pl(px(84.))
            .pr(px(12.))
            .bg(t.panel)
            .border_b_1()
            .border_color(t.line)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.))
                    .pr(px(14.))
                    .border_r_1()
                    .border_color(t.line)
                    .child(
                        div()
                            .text_size(metrics::TEXT_TITLE)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.ink)
                            .child("Settings"),
                    )
                    .child(
                        div()
                            .text_size(metrics::TEXT_SM)
                            .text_color(t.ink_3)
                            .child("saved as you change them"),
                    ),
            )
            .child(div().text_size(metrics::TEXT_SM).text_color(t.ink_3).child(
                if total_changed == 0 {
                    "All defaults".to_string()
                } else {
                    format!("{total_changed} customized")
                },
            ))
            .child(div().flex_1())
            .child(
                div().w(px(260.)).child(
                    Input::new(&self.search)
                        .prefix(icon("search", 13., t.ink_faint))
                        .h(px(30.)),
                ),
            )
            .child(
                div()
                    .id("only-changed")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(metrics::CONTROL_H)
                    .px(px(10.))
                    .rounded(metrics::RADIUS)
                    .border_1()
                    .border_color(if self.only_changed {
                        t.accent
                    } else {
                        t.field_border
                    })
                    .bg(if self.only_changed { t.sel_bg } else { t.panel })
                    .cursor_pointer()
                    .text_size(metrics::TEXT_SM)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if self.only_changed {
                        t.sel_text
                    } else {
                        t.ink_2
                    })
                    .child("Changed only")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.only_changed = !this.only_changed;
                        cx.notify();
                    })),
            )
            .child(
                ui::button("open-json", &t)
                    .text_size(metrics::TEXT_SM)
                    .child(icon("file", 12., t.ink))
                    .child("settings.json")
                    .on_click(|_, _, cx| {
                        // Make sure the file exists with every key before opening it.
                        settings::update(|_| {});
                        cx.open_url(&format!("file://{}", settings::path().display()));
                    }),
            )
            .child(
                ui::primary_button("settings-done", &t)
                    .child("Done")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Close))),
            );

        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .bg(t.ground)
            .occlude()
            .key_context("Settings")
            .on_key_down(cx.listener(|_, e: &gpui::KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" {
                    cx.emit(SettingsEvent::Close);
                }
            }))
            .child(header)
            .child(
                div()
                    .px(px(24.))
                    .py(px(12.))
                    .flex()
                    .justify_center()
                    .border_b_1()
                    .border_color(t.line)
                    .child(div().w(px(1024.)).child(pills)),
            )
            .child(
                div()
                    .id("settings-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(div().py(px(20.)).flex().justify_center().child(body))
                    .when_some(self.note.clone(), |d, n| {
                        d.child(
                            div()
                                .pb(px(20.))
                                .flex()
                                .justify_center()
                                .child(div().text_size(metrics::TEXT_SM).text_color(t.ok).child(n)),
                        )
                    }),
            )
    }
}
