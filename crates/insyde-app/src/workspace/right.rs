//! Right panel: Checks (PR + CI), Diff (changed files + patch), Editor (open file).

use super::{RightTab, Workspace};
use crate::ui::{self, icon};
use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, SharedString, Window, div, px, uniform_list};
use insyde_core::forge::CheckState;
use insyde_theme::{Theme, metrics};
use std::sync::Arc;

impl Workspace {
    pub(super) fn render_right(
        &mut self,
        t: &Theme,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ix = match self.right_tab {
            RightTab::Checks => 0,
            RightTab::Diff => 1,
            RightTab::Editor => 2,
        };
        let seg = ui::segmented(
            "right-seg",
            &["Checks", "Diff", "Editor"],
            ix,
            t,
            true,
            26.,
            {
                let e = cx.entity().downgrade();
                move |i, _, cx| {
                    let _ = e.update(cx, |this, cx| {
                        this.right_tab = [RightTab::Checks, RightTab::Diff, RightTab::Editor][i];
                        if i != 2 {
                            this.refresh_wt_details(cx);
                        }
                        cx.notify();
                    });
                }
            },
        );
        let body = match self.right_tab {
            RightTab::Checks => self.render_checks(t, cx),
            RightTab::Diff => self.render_diff(t, cx),
            RightTab::Editor => self.render_viewer(t),
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .bg(t.chrome)
            .border_l_1()
            .border_color(t.chrome_line)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .pt(px(10.))
                    .pb(px(8.))
                    .child(div().flex_1().child(seg))
                    .child(
                        ui::icon_button("pop-right", "popout", 13., t).on_click(cx.listener(
                            |this, _, _, cx| {
                                if let Some(url) =
                                    this.wt().and_then(|w| w.pr.as_ref()).map(|p| p.url.clone())
                                {
                                    cx.open_url(&url);
                                } else if let Some(p) = this.active_wt_path() {
                                    cx.open_url(&format!("file://{}", p.display()));
                                }
                            },
                        )),
                    ),
            )
            .child(body)
            .into_any_element()
    }

    fn render_checks(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(ws) = self.wt() else {
            return div().into_any_element();
        };
        let branch = self.active_branch();
        let base = self
            .project()
            .map(|p| p.project.base.clone())
            .unwrap_or_default();
        let Some(pr) = ws.pr.clone() else {
            let (files, a, r) = ws.files.iter().fold((0, 0, 0), |acc, f| {
                (acc.0 + 1, acc.1 + f.added, acc.2 + f.removed)
            });
            let loading = ws.loading_pr;
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .p(px(24.))
                .text_center()
                .child(div().size(px(26.)).rounded(px(5.)).bg(t.hover_2).flex().items_center().justify_center().text_size(metrics::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(t.ink_2).child("PR"))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(if loading { "Checking for a pull request…" } else { "No pull request yet" }))
                .child(div().max_w(px(240.)).text_size(metrics::TEXT_SM).text_color(t.ink_3).child(if files == 0 {
                    format!("{branch} has no changes against {base}.")
                } else {
                    format!("{files} changed files (+{a} −{r}) on {branch}. Create a PR to run checks.")
                }))
                .when(files > 0 && branch != base, |d| d.child(ui::primary_button("pr-empty", t).h(px(28.)).text_size(metrics::TEXT_SM).child("Create PR").on_click(cx.listener(|this, _, _, cx| this.create_or_open_pr(cx)))))
                .into_any_element();
        };
        let (ok, run, bad) = pr.counts();
        let total = pr.checks.len().max(1);
        let mut list = div().flex().flex_col().gap(px(1.)).p(px(8.));
        for (i, c) in pr.checks.iter().enumerate() {
            let (tint, glyph) = match c.state {
                CheckState::Ok => (t.palette.green, "check"),
                CheckState::Running => (t.palette.amber, "running"),
                CheckState::Failed => (t.err, "cross"),
                CheckState::Skipped => (t.ink_disabled, "minus"),
            };
            let hover = t.hover;
            let url = c.url.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("chk-{i}")))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(8.))
                    .py(px(7.))
                    .rounded(metrics::RADIUS)
                    .cursor_pointer()
                    .bg(if c.state == CheckState::Running {
                        t.hover
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(move |s| s.bg(hover))
                    .child(
                        div()
                            .size(px(16.))
                            .flex_none()
                            .rounded_full()
                            .bg(tint)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(glyph, 10., t.palette.white)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                ui::trunc(c.name.clone())
                                    .text_size(metrics::TEXT_SM)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(t.ink),
                            )
                            .child(
                                ui::trunc(c.detail.clone())
                                    .text_size(metrics::TEXT_XS)
                                    .text_color(t.ink_3),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child(c.duration.clone()),
                    )
                    .on_click(move |_, _, cx| {
                        if let Some(u) = &url {
                            cx.open_url(u);
                        }
                    }),
            );
        }
        let mut sub = format!("{ok} passed");
        if run > 0 {
            sub.push_str(&format!(" · {run} running"));
        }
        if bad > 0 {
            sub.push_str(&format!(" · {bad} failed"));
        }
        sub.push_str(&format!(" · base {}", pr.base));
        let footer = if bad > 0 || run > 0 {
            "Merge blocked until checks pass".to_string()
        } else if pr.draft {
            "Draft · mark ready for review on GitHub".to_string()
        } else if pr.mergeable == Some(false) {
            "Merge conflicts with base".to_string()
        } else {
            "All checks passed".to_string()
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .px(px(14.))
                    .pt(px(6.))
                    .pb(px(12.))
                    .border_b_1()
                    .border_color(t.line)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .child(
                                div()
                                    .size(px(26.))
                                    .rounded(px(5.))
                                    .bg(t.hover_2)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(metrics::TEXT_XS)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(t.ink_2)
                                    .child("PR"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        ui::trunc(format!("#{} · {}", pr.number, pr.branch))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(t.ink),
                                    )
                                    .child(
                                        div()
                                            .mt(px(1.))
                                            .text_size(metrics::TEXT_XS)
                                            .text_color(t.ink_3)
                                            .child(sub),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(12.))
                            .flex()
                            .h(px(4.))
                            .gap(px(2.))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .bg(t.line)
                            .when(ok > 0, |d| {
                                d.child(
                                    div()
                                        .flex_grow(1.)
                                        .flex_basis(gpui::relative(ok as f32 / total as f32))
                                        .bg(t.ok),
                                )
                            })
                            .when(run > 0, |d| {
                                d.child(
                                    div()
                                        .flex_grow(1.)
                                        .flex_basis(gpui::relative(run as f32 / total as f32))
                                        .bg(t.warn),
                                )
                            })
                            .when(bad > 0, |d| {
                                d.child(
                                    div()
                                        .flex_grow(1.)
                                        .flex_basis(gpui::relative(bad as f32 / total as f32))
                                        .bg(t.err),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .id("checks")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .child(
                div()
                    .p(px(10.))
                    .border_t_1()
                    .border_color(t.line)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child(footer),
                    )
                    .child(
                        ui::small_button("rerun", "Re-run", t)
                            .h(px(28.))
                            .on_click(cx.listener(|this, _, _, cx| this.rerun_checks(cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_diff(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(ws) = self.wt() else {
            return div().into_any_element();
        };
        let files = ws.files.clone();
        let sel = ws.diff_sel.clone();
        let patch = ws.patch.clone();
        let (a, r) = files
            .iter()
            .fold((0, 0), |acc, f| (acc.0 + f.added, acc.1 + f.removed));
        let mut list = div().flex().flex_col().gap(px(1.)).px(px(8.)).pb(px(6.));
        for (i, f) in files.iter().enumerate().take(300) {
            let on = sel.as_deref() == Some(f.path.as_str());
            let hover = t.hover;
            let p = f.path.clone();
            let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
            let dir = f
                .path
                .rsplit_once('/')
                .map(|(d, _)| d.to_string())
                .unwrap_or_default();
            list = list.child(
                div()
                    .id(SharedString::from(format!("df-{i}")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(28.))
                    .px(px(8.))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .bg(if on {
                        t.sel_bg
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(move |s| s.bg(hover))
                    .text_size(metrics::TEXT_SM)
                    .child(icon("file", 12., t.ink_3))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .gap(px(6.))
                            .child(div().flex_none().text_color(t.ink).child(name))
                            .child(
                                ui::trunc(dir)
                                    .text_color(t.ink_3)
                                    .text_size(metrics::TEXT_XS),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(metrics::TEXT_XS)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.ok)
                            .child(if f.binary {
                                "bin".into()
                            } else {
                                format!("+{}", f.added)
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(metrics::TEXT_XS)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.err)
                            .child(format!("−{}", f.removed)),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.select_diff_file(p.clone(), cx)),
                    ),
            );
        }
        let header = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(14.))
            .pb(px(8.))
            .text_size(metrics::TEXT_SM)
            .child(div().font_weight(FontWeight::MEDIUM).child(format!(
                "{} changed file{}",
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            )))
            .child(
                div()
                    .text_color(t.ok)
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("+{a}")),
            )
            .child(
                div()
                    .text_color(t.err)
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("−{r}")),
            );
        let mut col = div().flex().flex_col().flex_1().min_h_0().child(header);
        if files.is_empty() {
            col = col.child(
                div()
                    .px(px(14.))
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_3)
                    .child("No changes against the base branch."),
            );
        }
        col = col.child(
            div()
                .id("diff-files")
                .max_h(px(if patch.is_some() { 180. } else { 2000. }))
                .overflow_y_scroll()
                .child(list),
        );
        if let Some(lines) = patch {
            let path = sel.clone().unwrap_or_default();
            let commented: std::collections::HashSet<(i64, bool)> = self
                .wt()
                .map(|w| {
                    w.comments
                        .iter()
                        .filter(|c| c.path == path)
                        .map(|c| (c.line, c.side == "new"))
                        .collect()
                })
                .unwrap_or_default();
            col = col.child(
                patch_list(lines, commented, t, cx.entity().downgrade())
                    .border_t_1()
                    .border_color(t.line),
            );
        } else if sel.is_some() {
            col = col.child(
                div()
                    .p(px(14.))
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_3)
                    .child("Loading diff…"),
            );
        }
        col = col.child(self.render_comments(t, cx));
        col.into_any_element()
    }

    /// Draft box for the line being commented, and the pending comments with "Send to agent".
    fn render_comments(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(ws) = self.wt() else {
            return div().into_any_element();
        };
        let mut col = div().flex().flex_col().flex_none();
        if let Some((path, line, side, code, input)) = &ws.draft {
            let loc = if side == "old" {
                format!("{path} · removed line {line}")
            } else {
                format!("{path}:{line}")
            };
            col = col.child(
                div()
                    .p(px(10.))
                    .border_t_1()
                    .border_color(t.line)
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child(loc),
                    )
                    .child(
                        ui::trunc(code.clone())
                            .font_family(metrics::MONO_FONT)
                            .text_size(metrics::TEXT_MONO)
                            .text_color(t.ink_2),
                    )
                    .child(gpui_component::input::Input::new(input).h(px(30.)))
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .justify_end()
                            .child(ui::small_button("draft-cancel", "Cancel", t).on_click(
                                cx.listener(|this, _, _, cx| {
                                    if let Some(ws) = this.wt_mut() {
                                        ws.draft = None;
                                    }
                                    cx.notify();
                                }),
                            ))
                            .child(
                                ui::primary_button("draft-add", t)
                                    .h(px(26.))
                                    .px(px(10.))
                                    .text_size(metrics::TEXT_SM)
                                    .child("Add comment")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let body = this
                                            .wt()
                                            .and_then(|w| w.draft.as_ref())
                                            .map(|d| d.4.read(cx).value().to_string())
                                            .unwrap_or_default();
                                        this.add_comment(body, cx);
                                    })),
                            ),
                    ),
            );
        }
        let comments = ws.comments.clone();
        if !comments.is_empty() {
            let mut list = div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .max_h(px(160.))
                .overflow_hidden();
            for c in &comments {
                let id = c.id;
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(metrics::TEXT_XS)
                        .child(ui::dot(t.accent, 6.))
                        .child(div().flex_none().text_color(t.ink_3).child(format!(
                            "{}:{}",
                            c.path.rsplit('/').next().unwrap_or(&c.path),
                            c.line
                        )))
                        .child(ui::trunc(c.body.clone()).flex_1().text_color(t.ink_2))
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("rm-c-{id}")),
                                "close",
                                10.,
                                t,
                            )
                            .size(px(18.))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.delete_comment(id, cx)),
                            ),
                        ),
                );
            }
            col = col.child(
                div()
                    .p(px(10.))
                    .border_t_1()
                    .border_color(t.line)
                    .bg(t.panel_2)
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(list)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(metrics::TEXT_XS)
                                    .text_color(t.ink_3)
                                    .child(format!(
                                        "{} comment{}",
                                        comments.len(),
                                        if comments.len() == 1 { "" } else { "s" }
                                    )),
                            )
                            .child(
                                ui::primary_button("send-comments", t)
                                    .h(px(26.))
                                    .px(px(10.))
                                    .text_size(metrics::TEXT_SM)
                                    .child("Send to agent")
                                    .on_click(
                                        cx.listener(|this, _, w, cx| this.send_comments(w, cx)),
                                    ),
                            ),
                    ),
            );
        }
        col.into_any_element()
    }

    fn render_viewer(&mut self, t: &Theme) -> AnyElement {
        match self.wt().and_then(|w| w.editor.clone()) {
            Some(ed) => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(ed)
                .into_any_element(),
            None => div()
                .p(px(14.))
                .text_size(metrics::TEXT_SM)
                .text_color(t.ink_3)
                .child("Open a file from the sidebar's Files or Search tab. ⌘S saves.")
                .into_any_element(),
        }
    }
}

/// Virtualized, line-numbered diff. Clicking a line starts a review comment on it.
fn patch_list(
    lines: Arc<Vec<insyde_core::git::PatchLine>>,
    commented: std::collections::HashSet<(i64, bool)>,
    t: &Theme,
    ws: gpui::WeakEntity<Workspace>,
) -> gpui::Div {
    let t = t.clone();
    let n = lines.len();
    div().flex_1().min_h_0().bg(t.panel_2).child(
        uniform_list("patch", n, move |range, _, _| {
            range
                .map(|i| {
                    let l = &lines[i];
                    let text = l.text.as_str();
                    let (bg, fg) = if text.starts_with('+') && !text.starts_with("+++") {
                        (t.ok.opacity(0.12), t.ink)
                    } else if text.starts_with('-') && !text.starts_with("---") {
                        (t.err.opacity(0.12), t.ink)
                    } else if text.starts_with("@@") {
                        (t.accent.opacity(0.08), t.accent)
                    } else {
                        (gpui::transparent_black(), t.ink_2)
                    };
                    let key = match (l.new, l.old) {
                        (Some(n), _) => Some((n as i64, true)),
                        (None, Some(o)) => Some((o as i64, false)),
                        _ => None,
                    };
                    let has = key.is_some_and(|k| commented.contains(&k));
                    let num = l.new.or(l.old).map(|n| n.to_string()).unwrap_or_default();
                    let hover = t.hover;
                    let group = SharedString::from(format!("pl-{i}"));
                    let line = l.clone();
                    let ws = ws.clone();
                    div()
                        .id(("patch-line", i))
                        .group(group.clone())
                        .w_full()
                        .flex()
                        .items_center()
                        .h(px(19.))
                        .bg(bg)
                        .font_family(metrics::MONO_FONT)
                        .text_size(metrics::TEXT_MONO)
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .when(key.is_some(), |d| {
                            d.cursor_pointer().hover(move |s| s.bg(hover))
                        })
                        .child(
                            div()
                                .w(px(40.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap(px(4.))
                                .pr(px(8.))
                                .text_color(t.ink_faint)
                                .when(has, |d| d.child(ui::dot(t.accent, 6.)))
                                .when(!has && key.is_some(), |d| {
                                    d.child(
                                        div()
                                            .invisible()
                                            .group_hover(group.clone(), |s| s.visible())
                                            .text_color(t.accent)
                                            .child("+"),
                                    )
                                })
                                .child(num),
                        )
                        .child(
                            div()
                                .text_color(fg)
                                .child(SharedString::from(l.text.clone())),
                        )
                        .when(key.is_some(), |d| {
                            d.on_click(move |_, window, cx| {
                                let line = line.clone();
                                let _ =
                                    ws.update(cx, |this, cx| this.start_comment(line, window, cx));
                            })
                        })
                })
                .collect()
        })
        .size_full(),
    )
}
