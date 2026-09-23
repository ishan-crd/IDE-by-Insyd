//! Top bar: brand, Project Brain controls, activity summary, cost, run, layout, theme, PR.

use super::{BrainState, Workspace};
use crate::ui::{self, icon};
use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, Window, div, px};
use insyde_theme::{Theme, metrics};

impl Workspace {
    pub(super) fn render_top(&mut self, t: &Theme, _w: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (running, review) = self.counts(cx);
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
                .child(div().text_size(metrics::TEXT_SM).text_color(t.ink_3).child("Persistent project context from main"))
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
                        .child(div().ml_auto().font_weight(FontWeight::MEDIUM).text_color(t.ink).child(format!("{pct}%"))),
                )
                .child(div().h(px(3.)).rounded(px(3.)).bg(t.line).overflow_hidden().child(div().h_full().w(gpui::relative(*pct as f32 / 100.)).bg(t.primary).rounded(px(3.))))
                .into_any_element(),
            Some(BrainState::Ready { handle, updated }) => {
                let n = handle.node_count();
                let hover = t.hover;
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        ui::button("open-brain", t)
                            .pl(px(10.))
                            .pr(px(8.))
                            .child(brain_colored(t))
                            .child("Context")
                            .child(
                                div()
                                    .min_w(px(18.))
                                    .h(px(18.))
                                    .px(px(5.))
                                    .rounded(px(4.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(t.hover_2)
                                    .text_color(t.ink_2)
                                    .text_size(metrics::TEXT_XS)
                                    .font_weight(FontWeight::SEMIBOLD)
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
                    .child(div().text_size(metrics::TEXT_XS).text_color(t.ink_3).child(format!("Updated {}", insyde_core::git::ago(*updated))))
                    .into_any_element()
            }
        };
        let run = self.run_command();
        let has_pr = self.wt().and_then(|w| w.pr.as_ref()).map(|p| p.number);
        let dark = t.is_dark();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(metrics::TOPBAR_H)
            .pl(px(84.)) // native traffic lights sit here
            .pr(px(12.))
            .bg(t.panel)
            .border_b_1()
            .border_color(t.line)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(6.))
                    .pr(px(14.))
                    .mr(px(4.))
                    .border_r_1()
                    .border_color(t.line)
                    .child(div().text_size(metrics::TEXT_TITLE).font_weight(FontWeight::SEMIBOLD).text_color(t.ink).child("InsyDE"))
                    .child(div().text_size(metrics::TEXT_SM).text_color(t.ink_3).child("by Insyd")),
            )
            .child(div().ml(px(2.)).child(brain))
            .child(div().flex_1())
            .child(div().mr(px(6.)).text_size(metrics::TEXT_SM).text_color(t.ink_3).child(format!(
                "{running} agent{} running · {review} need{} review",
                if running == 1 { "" } else { "s" },
                if review == 1 { "s" } else { "" }
            )))
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(metrics::CONTROL_H)
                    .px(px(10.))
                    .border_1()
                    .border_color(t.field_border)
                    .rounded(metrics::RADIUS)
                    .text_size(metrics::TEXT_SM)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.ink_2)
                    .child(format!("${cost:.2}")),
            )
            .when_some(run, |d, (cmd, label)| {
                d.child(
                    ui::button("run", t)
                        .child(icon("play", 12., t.ink))
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.add_pane(Some(cmd.clone()), cx);
                        })),
                )
            })
            .child(ui::button("layout", t).w(metrics::CONTROL_H).px_0().justify_center().child(icon("layout", 15., t.ink)).on_click(cx.listener(|this, _, _, cx| {
                this.prefs.show_right = !this.prefs.show_right;
                this.save_prefs();
                cx.notify();
            })))
            .child(ui::button("theme", t).w(metrics::CONTROL_H).px_0().justify_center().child(icon(if dark { "sun" } else { "moon" }, 15., t.ink)).on_click(cx.listener(|this, _, _, cx| this.toggle_theme(cx))))
            .child(
                ui::primary_button("create-pr", t)
                    .ml(px(2.))
                    .child(icon("pr", 13., t.on_primary))
                    .child(match has_pr {
                        Some(n) => format!("Open PR #{n}"),
                        None => "Create PR".into(),
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.create_or_open_pr(cx))),
            )
            .into_any_element()
    }
}

/// The filled, three-color brain glyph shown once a brain exists.
fn brain_colored(t: &Theme) -> gpui::Div {
    let p = &t.palette;
    div()
        .relative()
        .size(px(15.))
        .flex_none()
        .child(icon("brain-links", 15., t.ink_3).absolute().top_0().left_0())
        .child(div().absolute().left(px(1.8)).top(px(2.3)).size(px(4.1)).rounded_full().bg(p.blue))
        .child(div().absolute().left(px(10.3)).top(px(2.3)).size(px(3.3)).rounded_full().bg(p.red))
        .child(div().absolute().left(px(5.9)).top(px(9.8)).size(px(4.1)).rounded_full().bg(p.green))
}
