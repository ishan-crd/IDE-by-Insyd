//! Center: agent tab strip (+ Agent picker, context meter, Hand off) and the active tab.

use super::{TabView, Workspace};
use crate::ui::{self, icon, monogram};
use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, SharedString, Window, div, px};
use insyde_core::agents::AgentSpec;
use insyde_theme::{Theme, metrics};

impl Workspace {
    pub(super) fn render_center(&mut self, t: &Theme, _w: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(ws) = self.wt() else {
            return div().size_full().bg(t.ground).into_any_element();
        };
        let active = ws.active;
        let mut strip = div().flex().items_stretch().min_w_0().overflow_hidden().flex_shrink(1.);
        let n_tabs = ws.tabs.len();
        for (i, tab) in ws.tabs.iter().enumerate() {
            let spec = AgentSpec::get(tab.agent);
            let is = i == active;
            let id = tab.id;
            let hover = t.hover;
            let running = tab.running(cx);
            let attention = matches!(&tab.view, TabView::Chat(c) if c.read(cx).needs_attention()) && !is;
            let h2 = t.hover_2;
            let ink = t.ink;
            strip = strip.child(
                div()
                    .id(SharedString::from(format!("tab-{id}")))
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .flex_shrink(1.)
                    .w(px(220.))
                    .min_w(px(110.))
                    .pl(px(12.))
                    .pr(px(10.))
                    .cursor_pointer()
                    .text_color(if is { t.ink } else { t.ink_3 })
                    .border_r_1()
                    .border_color(t.line_soft)
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        if let Some(ws) = this.wt_mut() {
                            ws.active = i;
                            if let Some(TabView::Chat(c)) = ws.tabs.get(i).map(|t| &t.view) {
                                let c = c.clone();
                                c.update(cx, |c, cx| {
                                    c.unseen = false;
                                    c.focus_composer(w, cx);
                                });
                            }
                        }
                        cx.notify();
                    }))
                    .child(monogram(spec, 18., t))
                    .child(ui::trunc(tab.title(cx)).flex_1().font_weight(if is { FontWeight::MEDIUM } else { FontWeight::NORMAL }))
                    .when(running, |d| d.child(ui::pulse(SharedString::from(format!("tr-{id}")), ui::dot(t.ok, 6.))))
                    .when(attention && !running, |d| d.child(ui::dot(t.accent, 6.)))
                    .when(n_tabs > 1, |d| {
                        d.child(
                            div()
                                .id(SharedString::from(format!("close-{id}")))
                                .size(px(16.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .text_color(t.ink_faint)
                                .hover(move |s| s.bg(h2).text_color(ink))
                                .child("×")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.close_tab(id, cx);
                                })),
                        )
                    })
                    .child(div().absolute().left_0().right_0().bottom(px(-1.)).h(px(2.)).bg(if is { t.accent } else { gpui::transparent_black() })),
            );
        }
        let h2 = t.hover_2;
        let add = div()
            .flex_none()
            .flex()
            .items_center()
            .pl(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(1.))
                    .p(px(2.))
                    .rounded(px(7.))
                    .bg(if self.menu_open { t.hover_2 } else { gpui::transparent_black() })
                    .child(
                        div()
                            .id("quick-add")
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .h(px(26.))
                            .px(px(8.))
                            .rounded(px(5.))
                            .cursor_pointer()
                            .text_color(t.ink_2)
                            .text_size(metrics::TEXT_SM)
                            .font_weight(FontWeight::MEDIUM)
                            .hover(move |s| s.bg(h2))
                            .child(icon("plus", 12., t.ink_2))
                            .child("Agent")
                            .on_click(cx.listener(|this, e: &gpui::ClickEvent, w, cx| this.add_agent(2, e.modifiers().platform, w, cx))),
                    )
                    .child(
                        div()
                            .id("agent-menu")
                            .w(px(22.))
                            .h(px(26.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(5.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(h2))
                            .child(icon("chevron-down", 12., t.ink_3))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.menu_open = !this.menu_open;
                                this.handoff = None;
                                cx.notify();
                            })),
                    ),
            );

        // Right side: context meter, Hand off, agent count, history.
        let chat = self.active_chat();
        let (used, size) = chat.as_ref().map(|c| c.read(cx).context_usage()).unwrap_or((0, 200_000));
        let pct = (used as f64 * 100. / size.max(1) as f64).min(100.) as f32;
        let hot = pct > 85.;
        let meter_color = if hot { t.err } else { t.ink_3 };
        let agents = n_tabs;
        let hov = t.hover;
        let right = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .whitespace_nowrap()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .px(px(4.))
                    .text_size(metrics::TEXT_XS)
                    .text_color(t.ink_3)
                    .child(div().w(px(44.)).h(px(4.)).rounded(px(4.)).bg(t.line).overflow_hidden().child(div().h_full().rounded(px(4.)).w(gpui::relative(pct / 100.)).bg(meter_color)))
                    .child(div().font_weight(FontWeight::MEDIUM).text_color(meter_color).child(format!("{} / {}", ui::fmt_tokens(used), ui::fmt_tokens(size)))),
            )
            .child(
                div()
                    .id("handoff-btn")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(26.))
                    .px(px(10.))
                    .rounded(metrics::RADIUS)
                    .border_1()
                    .border_color(t.field_border)
                    .bg(if self.handoff.is_some() { t.hover_2 } else { t.panel })
                    .cursor_pointer()
                    .text_size(metrics::TEXT_SM)
                    .font_weight(FontWeight::MEDIUM)
                    .hover(move |s| s.bg(hov))
                    .child(icon("handoff", 13., t.ink))
                    .child("Hand off")
                    .on_click(cx.listener(|this, _, w, cx| this.open_handoff(w, cx))),
            )
            .child(
                div()
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(4.))
                    .bg(t.hover_2)
                    .text_size(metrics::TEXT_XS)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.ink_2)
                    .child(format!("{agents} agent{}", if agents == 1 { "" } else { "s" })),
            )
            .child(ui::icon_button("history", "history", 14., t).on_click(cx.listener(|this, _, _, cx| {
                this.history_open = !this.history_open;
                cx.notify();
            })));

        let bar = div()
            .relative()
            .flex()
            .items_stretch()
            .h(metrics::TAB_H)
            .flex_none()
            .pl(px(4.))
            .pr(px(8.))
            .bg(t.panel)
            .border_b_1()
            .border_color(t.line)
            .text_size(metrics::TEXT_SM)
            .child(strip)
            .child(add)
            .child(div().flex_1().min_w(px(8.)))
            .child(right);

        let body: AnyElement = match ws.tabs.get(active).map(|t| &t.view) {
            Some(TabView::Chat(c)) => c.clone().into_any_element(),
            Some(TabView::Term(v)) => div().size_full().bg(t.panel_2).child(v.clone()).into_any_element(),
            None => div().into_any_element(),
        };
        let _ = self.menu_open;
        div().flex().flex_col().size_full().min_w_0().bg(t.ground).child(bar).child(div().flex_1().min_h_0().child(body)).into_any_element()
    }
}
