//! Bottom strip: Terminals / Logs / Problems, split panes with draggable dividers.

use super::{BottomTab, Drag, Workspace};
use crate::ui::{self, icon};
use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, MouseButton, SharedString, Window, div, px};
use insyde_theme::{Theme, metrics};

const PANE_DOTS: usize = 3;

impl Workspace {
    fn problems(&self, cx: &gpui::App) -> Vec<(String, String)> {
        let Some(ws) = self.wt() else { return vec![] };
        let mut out = vec![];
        for p in &ws.panes {
            let v = p.view.read(cx);
            for line in v.tail(300).lines() {
                let l = line.to_lowercase();
                if l.contains("error")
                    || l.contains("failed")
                    || l.contains("panicked")
                    || l.contains("warn")
                {
                    out.push((v.title.to_string(), line.trim().to_string()));
                }
            }
        }
        out.truncate(200);
        out
    }

    pub(super) fn render_bottom(
        &mut self,
        t: &Theme,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let problems = self.problems(cx);
        let n_panes = self.wt().map(|w| w.panes.len()).unwrap_or(0);
        let ix = match self.bottom_tab {
            BottomTab::Terminals => 0,
            BottomTab::Logs => 1,
            BottomTab::Problems => 2,
            BottomTab::Team => 3,
        };
        let seg = ui::segmented(
            "bottom-seg",
            &["Terminals", "Logs", "Problems", "Team"],
            ix,
            t,
            false,
            24.,
            {
                let e = cx.entity().downgrade();
                move |i, _, cx| {
                    let _ = e.update(cx, |this, cx| {
                        this.bottom_tab = [
                            BottomTab::Terminals,
                            BottomTab::Logs,
                            BottomTab::Problems,
                            BottomTab::Team,
                        ][i];
                        cx.notify();
                    });
                }
            },
        );
        let accent = t.accent;
        let dragging = self.is_dragging("term");
        let handle = div()
            .id("drag-term")
            .absolute()
            .left_0()
            .right_0()
            .top_0()
            .h(px(5.))
            .cursor_row_resize()
            .bg(if dragging {
                t.accent
            } else {
                gpui::transparent_black()
            })
            .hover(move |s| s.bg(accent))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &gpui::MouseDownEvent, _, cx| {
                    if e.click_count == 2 {
                        this.reset_sizes(cx);
                        return;
                    }
                    let h0 = this.prefs.term_h;
                    this.start_drag(Drag::Term {
                        y0: f32::from(e.position.y),
                        h0,
                    });
                    cx.notify();
                }),
            );
        let header = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(40.))
            .px(px(14.))
            .border_b_1()
            .border_color(t.line)
            .child(
                div()
                    .text_size(metrics::TEXT_SM)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.ink)
                    .child("Terminals"),
            )
            .child(
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_3)
                    .child(format!(
                        "{n_panes} session{} · {} problem{}",
                        if n_panes == 1 { "" } else { "s" },
                        problems.len(),
                        if problems.len() == 1 { "" } else { "s" }
                    )),
            )
            .child(div().flex_1())
            .child(seg)
            .child(ui::small_button("new-term", "New", t).on_click(cx.listener(
                |this, _, _, cx| {
                    this.bottom_tab = BottomTab::Terminals;
                    this.add_pane(None, cx);
                },
            )));

        let body: AnyElement = match self.bottom_tab {
            BottomTab::Terminals => self.render_panes(t, cx),
            BottomTab::Logs => {
                let mut col = div()
                    .flex()
                    .flex_col()
                    .px(px(14.))
                    .py(px(8.))
                    .font_family(metrics::MONO_FONT)
                    .text_size(metrics::TEXT_MONO);
                for l in self.logs.iter().rev().take(200) {
                    col = col.child(
                        div()
                            .text_color(t.ink_2)
                            .whitespace_nowrap()
                            .child(l.clone()),
                    );
                }
                if self.logs.is_empty() {
                    col = col.child(
                        div()
                            .text_color(t.ink_3)
                            .child("InsyDE activity (git, PRs, brain builds) appears here."),
                    );
                }
                div()
                    .id("logs")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .bg(t.inner(t.panel_2))
                    .child(col)
                    .into_any_element()
            }
            BottomTab::Team => self.render_team_tab(t, cx),
            BottomTab::Problems => {
                let mut col = div().flex().flex_col().px(px(14.)).py(px(8.)).gap(px(2.));
                for (src, line) in &problems {
                    let err = line.to_lowercase().contains("error")
                        || line.to_lowercase().contains("failed");
                    col = col.child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .text_size(metrics::TEXT_SM)
                            .child(ui::dot(if err { t.err } else { t.warn }, 6.).mt(px(6.)))
                            .child(div().flex_none().text_color(t.ink_3).child(src.clone()))
                            .child(
                                ui::trunc(line.clone())
                                    .font_family(metrics::MONO_FONT)
                                    .text_size(metrics::TEXT_MONO)
                                    .text_color(t.ink_2),
                            ),
                    );
                }
                if problems.is_empty() {
                    col = col.child(
                        div()
                            .text_size(metrics::TEXT_SM)
                            .text_color(t.ink_3)
                            .child("No errors or warnings in recent terminal output."),
                    );
                }
                div()
                    .id("problems")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .bg(t.inner(t.panel_2))
                    .child(col)
                    .into_any_element()
            }
        };
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .bg(if t.glass { t.panel_2 } else { t.panel })
            .border_t_1()
            .border_color(t.line)
            .when(t.glass, |d| {
                d.rounded(metrics::RADIUS_LG)
                    .border_1()
                    .border_color(t.chrome_line)
                    .shadow(t.pop_shadow(false))
            })
            .overflow_hidden()
            .child(header)
            .child(div().flex_1().min_h_0().flex().flex_col().child(body))
            .child(handle)
            .into_any_element()
    }

    fn render_panes(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(ws) = self.wt() else {
            return div().into_any_element();
        };
        let panes: Vec<(gpui::Entity<crate::terminal_view::TerminalView>, f32)> =
            ws.panes.iter().map(|p| (p.view.clone(), p.frac)).collect();
        if panes.is_empty() {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .bg(t.inner(t.panel_2))
                .child(
                    ui::small_button("first-term", "Open a terminal", t)
                        .on_click(cx.listener(|this, _, _, cx| this.add_pane(None, cx))),
                )
                .into_any_element();
        }
        let dots = [t.palette.blue, t.palette.amber, t.palette.green];
        let total: f32 = panes.iter().map(|p| p.1).sum();
        let width_total = self.window_size.0 - self.sizes().0 - self.sizes().1;
        let mut row = div().flex_1().min_h_0().flex();
        let n = panes.len();
        for (i, (view, frac)) in panes.into_iter().enumerate() {
            let (title, sub, exited) = {
                let v = view.read(cx);
                (v.title.clone(), v.sub.clone(), v.exited)
            };
            let v2 = view.clone();
            let accent = t.accent;
            let dragging = self.is_dragging(&format!("pane{i}"));
            let mut pane = div()
                .relative()
                .flex()
                .flex_col()
                .min_w_0()
                .min_h_0()
                .flex_basis(gpui::relative(frac / total))
                .flex_grow(1.)
                .border_r_1()
                .border_color(t.line)
                .bg(t.inner(t.panel_2))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .h(px(30.))
                        .pl(px(12.))
                        .pr(px(8.))
                        .border_b_1()
                        .border_color(t.line_soft)
                        .text_size(metrics::TEXT_SM)
                        .child(div().size(px(7.)).rounded(px(2.)).bg(dots[i % PANE_DOTS]))
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.ink)
                                .child(title),
                        )
                        .child(div().text_size(metrics::TEXT_XS).text_color(t.ink_3).child(
                            match exited {
                                Some(code) => SharedString::from(format!("exited {code}")),
                                None => sub,
                            },
                        ))
                        .child(div().flex_1())
                        .child(
                            div()
                                .id(SharedString::from(format!("pop-{i}")))
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .h(px(22.))
                                .px(px(7.))
                                .rounded(px(5.))
                                .cursor_pointer()
                                .text_size(metrics::TEXT_XS)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.ink_3)
                                .hover({
                                    let h = t.hover_2;
                                    move |s| s.bg(h)
                                })
                                .child(icon("popout", 11., t.ink_3))
                                .child("Pop out")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.pop_out(v2.clone(), cx);
                                    if let Some(ws) = this.wt_mut() {
                                        ws.panes.retain(|p| p.view != v2);
                                    }
                                    cx.notify();
                                })),
                        )
                        .when(n > 1, |d| {
                            d.child(
                                ui::icon_button(
                                    SharedString::from(format!("x-{i}")),
                                    "close",
                                    11.,
                                    t,
                                )
                                .size(px(22.))
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.close_pane(i, cx)),
                                ),
                            )
                        }),
                )
                .child(div().relative().flex_1().min_h_0().child(view));
            if i + 1 < n {
                pane = pane.child(
                    div()
                        .id(SharedString::from(format!("pane-drag-{i}")))
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right(px(-3.))
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
                                if let Some(ws) = this.wt() {
                                    let fr0 = (ws.panes[i].frac, ws.panes[i + 1].frac);
                                    this.start_drag(Drag::Pane {
                                        idx: i,
                                        x0: f32::from(e.position.x),
                                        fr0,
                                        width: width_total,
                                    });
                                }
                                cx.notify();
                            }),
                        ),
                );
            }
            row = row.child(pane);
        }
        row.into_any_element()
    }
}
