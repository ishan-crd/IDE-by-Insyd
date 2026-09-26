//! Top bar, in three columns over the panes below: traffic lights (sidebar); Project Brain
//! and the agent count / history / Hand off (agent area); cost, layout, theme and
//! the Run button with its menu (right panel).

use super::{BrainState, Workspace};
use crate::ui::{self, icon};
use gpui::prelude::*;
use gpui::{
    Anchor, AnyElement, Context, FontWeight, SharedString, Window, anchored, deferred, div, px,
};
use insyde_theme::{Theme, metrics};

impl Workspace {
    pub(super) fn render_top(
        &mut self,
        t: &Theme,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cost = self.store.total_cost();
        let brain: AnyElement = match self.project().map(|p| &p.brain) {
            None => div().into_any_element(),
            Some(BrainState::None) => div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    ui::button("create-brain", t)
                        .pl(px(10.))
                        .child(icon("brain", 15., t.ink))
                        .child("Create Brain")
                        .on_click(cx.listener(|this, _, _, cx| this.build_brain(cx))),
                )
                .child(
                    div()
                        .text_size(metrics::TEXT_SM)
                        .text_color(t.ink_3)
                        .child("Persistent project context from main"),
                )
                .into_any_element(),
            Some(BrainState::Building { pct, label }) => div()
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(5.))
                .w(px(240.))
                .h(metrics::CONTROL_H)
                .px(px(10.))
                .border_1()
                .border_color(t.field_border)
                .rounded(metrics::RADIUS)
                .child(
                    div()
                        .flex()
                        .text_size(metrics::TEXT_XS)
                        .text_color(t.ink_2)
                        .child(ui::trunc(label.clone()).flex_1())
                        .child(
                            div()
                                .ml_auto()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.ink)
                                .child(format!("{pct}%")),
                        ),
                )
                .child(
                    div()
                        .h(px(3.))
                        .rounded(px(3.))
                        .bg(t.line)
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .w(gpui::relative(*pct as f32 / 100.))
                                .bg(t.primary)
                                .rounded(px(3.)),
                        ),
                )
                .into_any_element(),
            Some(BrainState::Ready { handle, updated }) => {
                let n = handle.node_count();
                let hover = t.hover;
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("open-brain")
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .h(metrics::CONTROL_H)
                            .px(px(10.))
                            .rounded(metrics::RADIUS)
                            .cursor_pointer()
                            .text_color(t.ink)
                            .font_weight(FontWeight::MEDIUM)
                            .hover(move |s| s.bg(hover))
                            .child(icon("brain", 15., t.ink_2))
                            .child("Context")
                            .child(
                                div()
                                    .text_size(metrics::TEXT_XS)
                                    .text_color(t.ink_3)
                                    .child(n.to_string()),
                            )
                            .on_click(cx.listener(|this, _, w, cx| this.open_brain(w, cx))),
                    )
                    .child(
                        div()
                            .id("update-brain")
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .h(metrics::CONTROL_H)
                            .px(px(10.))
                            .rounded(metrics::RADIUS)
                            .cursor_pointer()
                            .text_color(t.ink_2)
                            .font_weight(FontWeight::MEDIUM)
                            .hover(move |s| s.bg(hover))
                            .child(icon("refresh", 13., t.ink_2))
                            .child("Update")
                            .on_click(cx.listener(|this, _, _, cx| this.build_brain(cx))),
                    )
                    .child(
                        div()
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child(format!("Updated {}", insyde_core::git::ago(*updated))),
                    )
                    .into_any_element()
            }
        };
        let session = self.render_session_controls(t, cx);
        let run_btn = self.render_run_button(t, cx);
        let dark = t.is_dark();
        // Three columns over the panes below (sidebar | agents | right panel),
        // so each group stays over its pane as the panes are resized.
        let (side_w, right_w, _) = self.sizes();
        let gap = if t.glass { super::GLASS_GAP } else { 0. };
        // Over the sidebar: empty, apart from the native traffic lights.
        let lead = div().flex_none().h_full().w(px(side_w.max(84.)));
        // Over the agent area: brain on the left, session controls flush with
        // the agent tab bar's right edge.
        let center = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(8.))
            .h_full()
            .pl(px(gap + 6.))
            .pr(px(gap + 8.))
            .child(div().flex().min_w_0().overflow_hidden().child(brain))
            .child(div().flex_1())
            .child(session);
        let tools = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_end()
            .gap(px(4.))
            .h_full()
            .pl(px(12.))
            .pr(px(12.))
            .when(right_w > 0., |d| d.w(px(right_w)))
            .child(
                div()
                    .mr(px(6.))
                    .text_size(metrics::TEXT_SM)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.ink_3)
                    .child(format!("${cost:.2}")),
            )
            .child(
                ui::icon_button("layout", "layout", 15., t)
                    .size(metrics::CONTROL_H)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.prefs.show_right = !this.prefs.show_right;
                        this.save_prefs();
                        cx.notify();
                    })),
            )
            .child(
                ui::icon_button("theme", if dark { "sun" } else { "moon" }, 15., t)
                    .size(metrics::CONTROL_H)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_theme(cx))),
            )
            .child(div().ml(px(4.)).child(run_btn));
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(metrics::TOPBAR_H)
            .bg(t.chrome)
            .border_b_1()
            .border_color(t.chrome_line)
            .child(lead)
            .child(center)
            .child(tools)
            .into_any_element()
    }
}

impl Workspace {
    /// Agent count, session history and Hand off for the active worktree,
    /// at the right end of the agent area. Popovers open under their buttons.
    fn render_session_controls(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(agents) = self
            .wt()
            .map(|ws| ws.tabs.iter().filter(|t| t.is_agent()).count())
        else {
            return div().into_any_element();
        };
        let handoff_pop = self
            .handoff
            .is_some()
            .then(|| popover(self.render_handoff(t, cx)));
        let history_pop = self
            .history_open
            .then(|| popover(self.render_history(t, cx)));
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .whitespace_nowrap()
            .child(
                div()
                    .mr(px(4.))
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_3)
                    .child(format!(
                        "{agents} agent{}",
                        if agents == 1 { "" } else { "s" }
                    )),
            )
            .child(
                div()
                    .relative()
                    .child(
                        ui::icon_button("history", "history", 15., t)
                            .size(metrics::CONTROL_H)
                            .when(self.history_open, |d| d.bg(t.hover_2))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.history_open = !this.history_open;
                                this.handoff = None;
                                cx.notify();
                            })),
                    )
                    .children(history_pop),
            )
            .child(
                div()
                    .relative()
                    .ml(px(4.))
                    .child(
                        ui::button("handoff-btn", t)
                            .when(self.handoff.is_some(), |d| d.bg(t.hover_2))
                            .text_size(metrics::TEXT_SM)
                            .child(icon("handoff", 13., t.ink))
                            .child("Hand off")
                            .on_click(cx.listener(|this, _, w, cx| this.open_handoff(w, cx))),
                    )
                    .children(handoff_pop),
            )
            .into_any_element()
    }
}

impl Workspace {
    /// Run split button: the main part runs the Run command in a new terminal,
    /// the chevron opens the Run menu.
    fn render_run_button(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let run = self.run_command();
        let label = run
            .as_ref()
            .map(|r| r.1.clone())
            .unwrap_or_else(|| "Run…".into());
        let menu = self
            .run_menu
            .is_some()
            .then(|| popover(self.render_run_menu(t, cx)));
        let hover = t.primary_hover;
        div()
            .relative()
            .flex_none()
            .ml(px(2.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(metrics::CONTROL_H)
                    .rounded(metrics::RADIUS)
                    .bg(t.primary)
                    .text_color(t.on_primary)
                    .font_weight(FontWeight::MEDIUM)
                    .child(
                        div()
                            .id("run")
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .h_full()
                            .pl(px(12.))
                            .pr(px(10.))
                            .rounded_l(metrics::RADIUS)
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .child(icon("play", 12., t.on_primary))
                            .child(label)
                            .on_click(cx.listener(move |this, _, w, cx| match &run {
                                Some((cmd, _)) => this.run_now(cmd.clone(), cx),
                                None => this.toggle_run_menu(w, cx),
                            })),
                    )
                    .child(div().w(px(1.)).h(px(16.)).bg(t.on_primary.opacity(0.2)))
                    .child(
                        div()
                            .id("run-menu")
                            .flex()
                            .items_center()
                            .justify_center()
                            .h_full()
                            .w(px(26.))
                            .rounded_r(metrics::RADIUS)
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .child(icon("chevron-down", 12., t.on_primary))
                            .on_click(cx.listener(|this, _, w, cx| this.toggle_run_menu(w, cx))),
                    ),
            )
            .children(menu)
            .into_any_element()
    }

    /// Commands to run in a new terminal: the detected one, the Run setting,
    /// saved quick commands, and an input for anything else.
    fn render_run_menu(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(input) = self.run_menu.clone() else {
            return div().into_any_element();
        };
        let current = self.run_command().map(|r| r.0);
        let detected = self.detected_run().map(|r| r.0);
        let custom = insyde_core::settings::get().run_command.trim().to_string();
        // (command, saved in the quick list)
        let quick = insyde_core::settings::get().quick_list();
        let mut rows: Vec<(String, bool)> = vec![];
        for c in detected
            .iter()
            .chain(Some(&custom).filter(|c| !c.is_empty()))
            .chain(quick.iter())
        {
            if !rows.iter().any(|r| &r.0 == c) {
                rows.push((c.clone(), quick.contains(c)));
            }
        }
        let mut list = div().flex().flex_col().gap(px(1.));
        if rows.is_empty() {
            list = list.child(
                div()
                    .px(px(8.))
                    .py(px(6.))
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_3)
                    .child("Nothing detected here. Type a command below."),
            );
        }
        for (i, (cmd, saved)) in rows.into_iter().enumerate() {
            let is_current = current.as_deref() == Some(cmd.as_str());
            let is_detected = detected.as_deref() == Some(cmd.as_str());
            let group = SharedString::from(format!("rm-{i}"));
            let (hover, ink_2, ink) = (t.hover, t.ink_2, t.ink);
            let (c_run, c_def, c_del) = (cmd.clone(), cmd.clone(), cmd.clone());
            list = list.child(
                div()
                    .id(SharedString::from(format!("rm-row-{i}")))
                    .group(group.clone())
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(30.))
                    .px(px(8.))
                    .rounded(metrics::RADIUS)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(icon("play", 10., t.ink_3))
                    .child(
                        ui::trunc(cmd.clone())
                            .flex_1()
                            .font_family(metrics::MONO_FONT)
                            .text_size(metrics::TEXT_SM)
                            .text_color(t.ink),
                    )
                    .when(is_detected && !is_current, |d| {
                        d.child(
                            div()
                                .text_size(metrics::TEXT_XS)
                                .text_color(t.ink_faint)
                                .child("detected"),
                        )
                    })
                    .child(if is_current {
                        div()
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child("Run button")
                            .into_any_element()
                    } else {
                        div()
                            .id(SharedString::from(format!("rm-def-{i}")))
                            .px(px(4.))
                            .rounded(px(4.))
                            .text_size(metrics::TEXT_XS)
                            .text_color(gpui::transparent_black())
                            .group_hover(group.clone(), move |s| s.text_color(ink_2))
                            .hover(move |s| s.text_color(ink))
                            .child("Make default")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.set_run_button(&c_def, cx);
                            }))
                            .into_any_element()
                    })
                    .when(saved, |d| {
                        d.child(
                            div()
                                .id(SharedString::from(format!("rm-del-{i}")))
                                .size(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .text_color(gpui::transparent_black())
                                .group_hover(group.clone(), move |s| s.text_color(ink_2))
                                .hover(move |s| s.text_color(ink))
                                .child("×")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.remove_quick(&c_del, cx);
                                })),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.run_now(c_run.clone(), cx))),
            );
        }
        let hover = t.hover;
        div()
            .w(px(340.))
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
                    .child("Run in a new terminal"),
            )
            .child(list)
            .child(
                div()
                    .mt(px(4.))
                    .px(px(4.))
                    .pt(px(8.))
                    .pb(px(2.))
                    .border_t_1()
                    .border_color(t.line_soft)
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(gpui_component::input::Input::new(&input).h(px(28.)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .px(px(4.))
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child("Enter runs it and saves it here")
                            .child(
                                div()
                                    .id("rm-settings")
                                    .ml_auto()
                                    .px(px(4.))
                                    .rounded(px(4.))
                                    .cursor_pointer()
                                    .hover(move |s| s.bg(hover))
                                    .child("Edit in Settings")
                                    .on_click(cx.listener(|this, _, w, cx| {
                                        this.run_menu = None;
                                        this.open_settings(w, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}

/// Floats `content` under the bottom-right corner of its (relative) parent,
/// painted above the rest of the window.
fn popover(content: AnyElement) -> impl IntoElement {
    div().absolute().right_0().bottom_0().child(
        deferred(
            anchored()
                .anchor(Anchor::TopRight)
                .snap_to_window_with_margin(px(8.))
                .child(div().pt(px(6.)).child(content)),
        )
        .with_priority(1),
    )
}
