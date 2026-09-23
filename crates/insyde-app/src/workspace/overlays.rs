//! Overlays: agent picker, hand-off popover, session history, brain screen,
//! toasts, the status bar and the first-run welcome.

use super::{Drag, Workspace};
use crate::ui::{self, icon, monogram};
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, FontWeight, MouseButton, SharedString, Stateful, Window, div, px,
};
use insyde_core::agents::{AGENTS, AgentSpec};
use insyde_theme::{Theme, metrics};

impl Workspace {
    /// Vertical resize handle at `left` (or `right`) px inside the middle row.
    pub(super) fn handle_v(
        &mut self,
        which: &'static str,
        left: f32,
        right: Option<f32>,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let accent = t.accent;
        let dragging = self.is_dragging(which);
        let mut d = div()
            .id(which)
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(5.))
            .cursor_col_resize()
            .bg(if dragging {
                t.accent
            } else {
                gpui::transparent_black()
            })
            .hover(move |s| s.bg(accent))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                    if e.click_count == 2 {
                        this.reset_sizes(cx);
                        return;
                    }
                    let x0 = f32::from(e.position.x);
                    let d = if which == "side" {
                        Drag::Side {
                            x0,
                            w0: this.prefs.side_w,
                        }
                    } else {
                        Drag::Right {
                            x0,
                            w0: this.prefs.right_w,
                        }
                    };
                    this.start_drag(d);
                    cx.notify();
                }),
            );
        d = match right {
            Some(r) => d.right(px(r)),
            None => d.left(px(left)),
        };
        d
    }

    pub(super) fn render_overlays(
        &mut self,
        mut root: Stateful<Div>,
        t: &Theme,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let side_w = self.sizes().0;
        let tabs_w = self.wt().map(|w| w.tabs.len()).unwrap_or(1) as f32;
        let top = f32::from(metrics::TOPBAR_H) + f32::from(metrics::TAB_H) - 2.;
        if self.menu_open {
            let branch = self.active_branch();
            let mut menu = div()
                .absolute()
                .top(px(top))
                .left(px(side_w + (4. + tabs_w * 220.).min(560.) - 20.))
                .w(px(252.))
                .p(px(4.))
                .bg(t.panel)
                .border_1()
                .border_color(t.line)
                .rounded(metrics::RADIUS_LG)
                .shadow(t.pop_shadow(true))
                .occlude()
                .child(
                    div()
                        .px(px(8.))
                        .pt(px(6.))
                        .pb(px(4.))
                        .text_size(metrics::TEXT_XS)
                        .text_color(t.ink_3)
                        .child(format!("Add to {branch}")),
                );
            let default_i = Self::default_agent_index();
            for (i, spec) in AGENTS.iter().enumerate() {
                let hover = t.hover;
                let key = if spec.is_tool() {
                    String::new()
                } else {
                    (i + 1).to_string()
                };
                let installed = spec.is_tool()
                    || spec.acp.map(|c| AgentSpec::available(&c)).unwrap_or(false)
                    || spec.tui.map(|c| AgentSpec::available(&c)).unwrap_or(false);
                let hint = if i == default_i {
                    "default"
                } else if !installed {
                    "not installed"
                } else if spec.acp.is_none() && !spec.is_tool() {
                    "terminal"
                } else {
                    ""
                };
                menu = menu.child(
                    div()
                        .id(SharedString::from(format!("am-{i}")))
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .h(px(32.))
                        .px(px(8.))
                        .rounded(metrics::RADIUS)
                        .cursor_pointer()
                        .bg(if i == default_i {
                            t.hover_2
                        } else {
                            gpui::transparent_black()
                        })
                        .hover(move |s| s.bg(hover))
                        .child(
                            div()
                                .w(px(12.))
                                .text_right()
                                .text_size(metrics::TEXT_XS)
                                .text_color(t.ink_faint)
                                .child(key),
                        )
                        .child(monogram(spec, 20., t))
                        .child(
                            div()
                                .flex_1()
                                .text_color(if installed { t.ink } else { t.ink_3 })
                                .child(spec.name),
                        )
                        .child(
                            div()
                                .text_size(metrics::TEXT_XS)
                                .text_color(t.ink_3)
                                .child(hint),
                        )
                        .on_click(cx.listener(move |this, e: &gpui::ClickEvent, w, cx| {
                            this.add_agent(i, e.modifiers().platform, w, cx)
                        })),
                );
            }
            let hover = t.hover;
            menu = menu.child(
                div()
                    .id("am-team")
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(32.))
                    .px(px(8.))
                    .rounded(metrics::RADIUS)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(div().w(px(12.)))
                    .child(
                        div()
                            .size(px(20.))
                            .rounded_full()
                            .bg(t.hover_2)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(10.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(t.ink_2)
                            .child("T"),
                    )
                    .child(div().flex_1().child("Team…"))
                    .child(
                        div()
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child("lead + specialists"),
                    )
                    .on_click(cx.listener(|this, _, w, cx| this.open_team_form(w, cx))),
            );
            // How new agents start: remembered in settings, changeable right here.
            let st = insyde_core::settings::get();
            use insyde_core::settings::{Approval, OpenAs};
            menu = menu.child(
                div()
                    .mt(px(4.))
                    .px(px(8.))
                    .pt(px(8.))
                    .pb(px(2.))
                    .border_t_1()
                    .border_color(t.line_soft)
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(self.menu_seg(
                        "Open in",
                        "am-open",
                        &[("Chat", OpenAs::Chat), ("Terminal", OpenAs::Terminal)],
                        st.open_agents_as,
                        |s, v| s.open_agents_as = v,
                        t,
                        cx,
                    ))
                    .child(self.menu_seg(
                        "Permissions",
                        "am-perm",
                        &[
                            ("Ask", Approval::Ask),
                            ("Edits", Approval::AcceptEdits),
                            ("Skip", Approval::FullAccess),
                        ],
                        st.approval,
                        |s, v| s.approval = v,
                        t,
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child(if st.approval == Approval::FullAccess {
                                "No permission prompts"
                            } else {
                                "⌘-click: other mode"
                            })
                            .child(div().ml_auto().child("1–9 to pick")),
                    ),
            );
            root = root
                .child(scrim("menu-scrim", cx, |this| this.menu_open = false))
                .child(menu);
        }

        if let Some(input) = self.connect_form.clone() {
            root = root.child(scrim("connect-scrim", cx, |this| this.connect_form = None)).child(
                div()
                    .absolute()
                    .left(px(10.))
                    .bottom(px(f32::from(metrics::STATUS_H) + self.sizes().2 + 48.))
                    .w(px(320.))
                    .p(px(10.))
                    .bg(t.panel)
                    .border_1()
                    .border_color(t.line)
                    .rounded(metrics::RADIUS_LG)
                    .shadow(t.pop_shadow(true))
                    .occlude()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(div().text_size(metrics::TEXT).font_weight(FontWeight::SEMIBOLD).child("Open a project"))
                    .child(
                        ui::button("open-folder", t)
                            .h(px(28.))
                            .text_size(metrics::TEXT_SM)
                            .child(icon("folder", 13., t.ink))
                            .child("Local folder…")
                            .on_click(cx.listener(|this, _, w, cx| this.add_project(w, cx))),
                    )
                    .child(div().text_size(metrics::TEXT_XS).text_color(t.ink_3).child("Or connect over SSH (uses your ~/.ssh/config and keys):"))
                    .child(gpui_component::input::Input::new(&input).h(px(30.)))
                    .child(div().text_size(metrics::TEXT_XS).text_color(t.ink_faint).child("Git, terminals and agents run on the host. Press Enter to connect.")),
            );
        }

        if self.team_form.is_some() {
            root = root
                .child(scrim("team-scrim", cx, |this| this.team_form = None))
                .child(self.render_team_form(t, cx));
        }

        if self.handoff.is_some() {
            root = root
                .child(scrim("ho-scrim", cx, |this| this.handoff = None))
                .child(self.render_handoff(t, top, cx));
        }

        if self.history_open {
            root = root
                .child(scrim("hist-scrim", cx, |this| this.history_open = false))
                .child(self.render_history(t, top, cx));
        }

        if let Some(bv) = &self.brain_view {
            root = root.child(bv.clone());
        }
        if let Some(sv) = &self.settings_view {
            root = root.child(sv.clone());
        }

        if let Some(toast) = &self.toast {
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(40.))
                    .right(px(16.))
                    .max_w(px(420.))
                    .px(px(12.))
                    .py(px(9.))
                    .rounded(px(8.))
                    .bg(if toast.error { t.err_bg } else { t.panel })
                    .border_1()
                    .border_color(if toast.error { t.err_border } else { t.line })
                    .shadow(t.pop_shadow(true))
                    .text_size(metrics::TEXT_SM)
                    .text_color(if toast.error { t.err } else { t.ink })
                    .child(toast.text.clone()),
            );
        }
        root
    }

    fn render_handoff(&mut self, t: &Theme, top: f32, cx: &mut Context<Self>) -> AnyElement {
        let Some(h) = &self.handoff else {
            return div().into_any_element();
        };
        let branch = self.active_branch();
        let mut grid = div().flex().flex_wrap().gap(px(4.));
        for (i, spec) in AGENTS.iter().enumerate().take(9) {
            let on = i == h.target;
            let hover = t.hover;
            grid = grid.child(
                div()
                    .id(SharedString::from(format!("ho-a-{i}")))
                    .w(px(102.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .h(px(32.))
                    .px(px(8.))
                    .rounded(metrics::RADIUS)
                    .cursor_pointer()
                    .text_size(metrics::TEXT_SM)
                    .bg(if on {
                        t.sel_bg
                    } else {
                        gpui::transparent_black()
                    })
                    .border_1()
                    .border_color(if on { t.sel_chip } else { t.line_soft })
                    .hover(move |s| s.bg(hover))
                    .when(spec.acp.is_none(), |d| d.opacity(0.5))
                    .child(monogram(spec, 18., t))
                    .child(ui::trunc(spec.name))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(h) = &mut this.handoff
                            && AGENTS[i].acp.is_some()
                        {
                            h.target = i;
                        }
                        cx.notify();
                    })),
            );
        }
        let labels = [
            "Conversation summary",
            "Changed files & diff",
            "Terminal & test state",
            "Open tasks",
            "Project Brain",
        ];
        let mut opts = div().flex().flex_col();
        let mut total = 0.;
        for (i, label) in labels.iter().enumerate() {
            let on = h.opts[i];
            if on {
                total += h.tokens[i];
            }
            let hover = t.hover;
            opts = opts.child(
                div()
                    .id(SharedString::from(format!("ho-o-{i}")))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .py(px(5.))
                    .px(px(6.))
                    .rounded(metrics::RADIUS)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(ui::checkbox(on, t))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(metrics::TEXT_SM)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(*label),
                            )
                            .child(
                                ui::trunc(h.subs[i].clone())
                                    .text_size(metrics::TEXT_XS)
                                    .text_color(t.ink_3),
                            ),
                    )
                    .child(
                        div()
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child(ui::fmt_k(h.tokens[i])),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(h) = &mut this.handoff
                            && h.tokens[i] > 0.
                        {
                            h.opts[i] = !h.opts[i];
                        }
                        cx.notify();
                    })),
            );
        }
        let target = AGENTS.get(h.target).map(|s| s.name).unwrap_or("agent");
        div()
            .absolute()
            .top(px(top))
            .right(px(self.sizes().1 + 150.))
            .w(px(340.))
            .bg(t.panel)
            .border_1()
            .border_color(t.line)
            .rounded(metrics::RADIUS_LG)
            .shadow(t.pop_shadow(true))
            .overflow_hidden()
            .occlude()
            .child(
                div()
                    .px(px(14.))
                    .pt(px(12.))
                    .pb(px(10.))
                    .border_b_1()
                    .border_color(t.line)
                    .child(div().text_size(metrics::TEXT).font_weight(FontWeight::SEMIBOLD).child("Hand off session"))
                    .child(div().mt(px(2.)).text_size(metrics::TEXT_XS).text_color(t.ink_3).child(format!("Start a fresh agent in {branch} with this session's context carried over."))),
            )
            .child(div().px(px(10.)).pt(px(10.)).pb(px(6.)).child(div().px(px(4.)).pb(px(6.)).text_size(metrics::TEXT_XS).text_color(t.ink_3).child("Continue with")).child(grid))
            .child(div().px(px(10.)).pt(px(6.)).pb(px(8.)).child(div().px(px(4.)).pb(px(4.)).text_size(metrics::TEXT_XS).text_color(t.ink_3).child("Carry over")).child(opts))
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
                    .child(ui::trunc(format!("{} → fresh window", ui::fmt_k(total))).flex_1().text_size(metrics::TEXT_XS).text_color(t.ink_3))
                    .child(ui::small_button("ho-cancel", "Cancel", t).h(px(28.)).on_click(cx.listener(|this, _, _, cx| {
                        this.handoff = None;
                        cx.notify();
                    })))
                    .child(ui::primary_button("ho-go", t).h(px(28.)).px(px(12.)).text_size(metrics::TEXT_SM).child(format!("Hand off to {target}")).on_click(cx.listener(|this, _, w, cx| this.do_handoff(w, cx)))),
            )
            .into_any_element()
    }

    fn render_history(&mut self, t: &Theme, top: f32, cx: &mut Context<Self>) -> AnyElement {
        let path = self.active_wt_path();
        let rows = path
            .as_ref()
            .map(|p| self.store.recent_sessions(p, 30))
            .unwrap_or_default();
        let open: Vec<i64> = self
            .wt()
            .map(|ws| {
                ws.tabs
                    .iter()
                    .filter_map(|t| {
                        if let super::TabView::Chat(c) = &t.view {
                            c.read(cx).session_row()
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut list = div().flex().flex_col().gap(px(1.));
        if rows.is_empty() {
            list = list.child(
                div()
                    .p(px(10.))
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_3)
                    .child("No sessions in this worktree yet."),
            );
        }
        for r in rows {
            let is_open = open.contains(&r.id);
            let spec = AgentSpec::by_key(&r.agent).unwrap_or(&AGENTS[2]);
            let hover = t.hover;
            list = list.child(
                div()
                    .id(SharedString::from(format!("hs-{}", r.id)))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(32.))
                    .px(px(8.))
                    .rounded(metrics::RADIUS)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(monogram(spec, 18., t))
                    .child(
                        ui::trunc(r.title.clone())
                            .flex_1()
                            .text_size(metrics::TEXT_SM),
                    )
                    .child(div().text_size(metrics::TEXT_XS).text_color(t.ink_3).child(
                        if is_open {
                            "open".to_string()
                        } else {
                            format!("${:.2}", r.cost)
                        },
                    ))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.history_open = false;
                        if !is_open {
                            this.store.reopen_session(r.id);
                            this.open_chat(spec.id, Some(r.clone()), None, w, cx);
                        }
                        cx.notify();
                    })),
            );
        }
        div()
            .absolute()
            .top(px(top))
            .right(px(self.sizes().1 + 8.))
            .w(px(300.))
            .max_h(px(420.))
            .p(px(4.))
            .bg(t.panel)
            .border_1()
            .border_color(t.line)
            .rounded(metrics::RADIUS_LG)
            .shadow(t.pop_shadow(true))
            .occlude()
            .child(
                div()
                    .px(px(8.))
                    .pt(px(6.))
                    .pb(px(4.))
                    .text_size(metrics::TEXT_XS)
                    .text_color(t.ink_3)
                    .child("Session history"),
            )
            .child(
                div()
                    .id("hist-list")
                    .max_h(px(380.))
                    .overflow_y_scroll()
                    .child(list),
            )
            .into_any_element()
    }

    pub(super) fn render_status(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let branch = self.active_branch();
        let ab = self.active_wt_path().and_then(|p| self.ab_cache(&p));
        let pr = self
            .wt()
            .and_then(|w| w.pr.as_ref())
            .map(|p| {
                format!(
                    "PR #{} {}",
                    p.number,
                    if p.draft { "draft" } else { "open" }
                )
            })
            .unwrap_or_else(|| "No PR".into());
        let (running, _) = self.counts(cx);
        let detached = cx.windows().len().saturating_sub(1);
        let lang = self
            .wt()
            .and_then(|w| w.editor.as_ref())
            .map(|e| {
                e.read(cx)
                    .rel
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_uppercase()
            })
            .filter(|s| !s.is_empty() && s.len() < 6);
        let stack = self
            .project()
            .map(|p| p.project.stack.clone())
            .unwrap_or_default();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(16.))
            .h(metrics::STATUS_H)
            .px(px(14.))
            .bg(t.chrome)
            .border_t_1()
            .border_color(t.chrome_line)
            .text_size(metrics::TEXT_XS)
            .text_color(t.ink_3)
            .child(
                div()
                    .text_color(t.ink_2)
                    .font_weight(FontWeight::MEDIUM)
                    .child(branch),
            )
            .child(
                ab.map(|(a, b)| format!("↑{a} ↓{b}"))
                    .unwrap_or_else(|| "no upstream".into()),
            )
            .child(pr)
            .child(format!("Agents: {running} running"))
            .child(div().ml_auto().child(format!(
                "{}×{}",
                self.window_size.0 as i32, self.window_size.1 as i32
            )))
            .child(format!(
                "{detached} window{} detached",
                if detached == 1 { "" } else { "s" }
            ))
            .child(format!("UTF-8 · {}", lang.unwrap_or(stack)))
            .into_any_element()
    }

    fn ab_cache(&self, path: &std::path::Path) -> Option<(u32, u32)> {
        self.ahead_behind.get(path).copied()
    }

    pub(super) fn render_welcome(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child("Open a repository"))
            .child(div().max_w(px(380.)).text_center().text_size(metrics::TEXT_SM).text_color(t.ink_3).child("InsyDE runs coding agents in isolated git worktrees. Pick a git repository to get started."))
            .child(ui::primary_button("open-repo", t).child(icon("folder", 13., t.on_primary)).child("Open repository…").on_click(cx.listener(|this, _, w, cx| this.add_project(w, cx))))
            .into_any_element()
    }
}

/// Full-window click-catcher that closes a popover.
fn scrim(
    id: &'static str,
    cx: &mut Context<Workspace>,
    close: impl Fn(&mut Workspace) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .inset_0()
        .on_click(cx.listener(move |this, _, _, cx| {
            close(this);
            cx.notify();
        }))
}

impl Workspace {
    /// A labelled segmented control in the agent menu, bound to one setting.
    #[allow(clippy::too_many_arguments)]
    fn menu_seg<T: Copy + PartialEq + 'static>(
        &self,
        label: &'static str,
        id: &'static str,
        opts: &[(&'static str, T)],
        cur: T,
        set: fn(&mut insyde_core::settings::Settings, T),
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut seg = div()
            .flex()
            .p(px(2.))
            .gap(px(2.))
            .rounded(metrics::RADIUS)
            .bg(t.hover);
        for (i, (name, v)) in opts.iter().copied().enumerate() {
            let on = v == cur;
            seg = seg.child(
                div()
                    .id(SharedString::from(format!("{id}-{i}")))
                    .px(px(8.))
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_size(metrics::TEXT_XS)
                    .font_weight(FontWeight::MEDIUM)
                    .when(on, |d| {
                        d.bg(t.seg_active).shadow(t.seg_shadow()).text_color(t.ink)
                    })
                    .when(!on, |d| d.text_color(t.ink_3))
                    .child(name)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        insyde_core::settings::update(|s| set(s, v));
                        cx.notify();
                    })),
            );
        }
        div()
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_size(metrics::TEXT_XS)
                    .text_color(t.ink_3)
                    .child(label),
            )
            .child(seg)
            .into_any_element()
    }
}
