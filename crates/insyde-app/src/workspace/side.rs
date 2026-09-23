//! Sidebar: Worktrees / Files / Search, project header, swipe between projects.

use super::{SideTab, Workspace};
use crate::ui::{self, icon};
use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, SharedString, Window, div, px};
use gpui_component::input::Input;
use insyde_core::project::WtStatus;
use insyde_theme::{Theme, metrics};
use std::path::PathBuf;

impl Workspace {
    pub(super) fn render_side(
        &mut self,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tab_ix = match self.side_tab {
            SideTab::Worktrees => 0,
            SideTab::Files => 1,
            SideTab::Search => 2,
        };
        let seg = ui::segmented(
            "side-seg",
            &["Worktrees", "Files", "Search"],
            tab_ix,
            t,
            true,
            26.,
            {
                let e = cx.entity().downgrade();
                move |i, _, cx| {
                    let _ = e.update(cx, |this, cx| {
                        this.side_tab = [SideTab::Worktrees, SideTab::Files, SideTab::Search][i];
                        cx.notify();
                    });
                }
            },
        );
        let body = match self.side_tab {
            SideTab::Worktrees => self.render_worktrees(t, window, cx),
            SideTab::Files => self.render_files(t, cx),
            SideTab::Search => self.render_search(t, cx),
        };
        let n = self.projects.len();
        let mut dots = div()
            .flex_1()
            .flex()
            .justify_center()
            .items_center()
            .gap(px(6.));
        for i in 0..n {
            let on = i == self.p;
            dots = dots.child(
                div()
                    .id(SharedString::from(format!("dot-{i}")))
                    .w(px(if on { 16. } else { 6. }))
                    .h(px(6.))
                    .rounded(px(3.))
                    .cursor_pointer()
                    .bg(if on { t.ink } else { t.ink_disabled })
                    .on_click(cx.listener(move |this, _, w, cx| this.select_project(i, w, cx))),
            );
        }
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .bg(t.panel)
            .border_r_1()
            .border_color(t.line)
            .overflow_hidden()
            .child(div().px(px(10.)).pt(px(10.)).pb(px(8.)).child(seg))
            .child(body)
            .child(
                div()
                    .px(px(10.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(t.line)
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        ui::icon_button("add-project", "folder", 14., t).on_click(cx.listener(
                            |this, _, w, cx| {
                                if this.connect_form.is_some() {
                                    this.connect_form = None;
                                    cx.notify();
                                } else {
                                    this.open_connect_form(w, cx);
                                }
                            },
                        )),
                    )
                    .child(dots)
                    .child(
                        div()
                            .pr(px(4.))
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_faint)
                            .child(if n > 1 { "⇆ swipe" } else { "" }),
                    ),
            )
            .into_any_element()
    }

    fn render_worktrees(
        &mut self,
        t: &Theme,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(ps) = self.project() else {
            return div().into_any_element();
        };
        let live = self.live_worktrees(cx);
        let proj_name = ps.project.name.clone();
        let letter = ps.project.letter();
        let stack = ps.project.stack.clone();
        let active = ps.active_wt;
        let wts = ps.project.worktrees.clone();
        let scanning = ps.scanning && wts.is_empty();
        let dx = self.swipe_dx;
        let mut list = div().flex().flex_col().gap(px(1.)).px(px(8.));
        if let Some(input) = &self.new_wt {
            list = list.child(
                div()
                    .px(px(4.))
                    .pb(px(6.))
                    .child(Input::new(input).h(px(30.))),
            );
        }
        for (i, w) in wts.iter().enumerate() {
            let is_active = i == active;
            let is_live = live.contains(&w.path);
            let color = if is_live {
                t.ok
            } else if w.status == WtStatus::Pr {
                t.accent
            } else {
                t.ink_3
            };
            let hover = t.hover;
            let path = w.path.clone();
            let confirm = self.confirm_delete.as_ref() == Some(&w.path);
            let add = if w.stat.added > 0 {
                format!("+{}", w.stat.added)
            } else {
                String::new()
            };
            let del = if w.stat.removed > 0 {
                format!("−{}", w.stat.removed)
            } else {
                String::new()
            };
            let mut glyph = div()
                .relative()
                .size(px(16.))
                .flex_none()
                .child(icon("branch", 16., color));
            if w.status == WtStatus::Warn {
                glyph = glyph.child(
                    div()
                        .absolute()
                        .top(px(-3.))
                        .right(px(-3.))
                        .size(px(7.))
                        .rounded_full()
                        .bg(t.err)
                        .border_2()
                        .border_color(t.panel),
                );
            }
            if is_live {
                glyph = glyph.child(ui::pulse(
                    SharedString::from(format!("live-{i}")),
                    div()
                        .absolute()
                        .top(px(-3.))
                        .right(px(-3.))
                        .size(px(7.))
                        .rounded_full()
                        .bg(t.ok)
                        .border_2()
                        .border_color(t.panel),
                ));
            }
            let group = SharedString::from(format!("wt-{i}"));
            let row = div()
                .id(SharedString::from(format!("wt-{i}")))
                .group(group.clone())
                .flex()
                .gap(px(8.))
                .px(px(8.))
                .py(px(6.))
                .rounded(metrics::RADIUS)
                .cursor_pointer()
                .bg(if is_active {
                    t.sel_bg
                } else {
                    gpui::transparent_black()
                })
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(move |this, _, w, cx| this.select_wt(i, w, cx)))
                .child(
                    div()
                        .w(px(12.))
                        .pt(px(2.))
                        .text_right()
                        .text_size(metrics::TEXT_XS)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.ink_4)
                        .child((i + 1).to_string()),
                )
                .child(div().pt(px(1.)).child(glyph))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .text_size(metrics::TEXT_SM)
                                .font_weight(if is_active {
                                    FontWeight::MEDIUM
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(if is_active { t.ink } else { t.ink_2 })
                                .child(ui::trunc(w.branch.clone()))
                                .when(w.primary, |d| {
                                    d.child(
                                        div().text_size(px(10.)).text_color(t.ink_faint).child("★"),
                                    )
                                })
                                .child(div().flex_1())
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(5.))
                                        .flex_none()
                                        .text_size(metrics::TEXT_XS)
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(div().text_color(t.ok).child(add))
                                        .child(div().text_color(t.err).child(del)),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .text_size(metrics::TEXT_XS)
                                .text_color(if confirm { t.err } else { t.ink_3 })
                                .child(
                                    ui::trunc(if confirm {
                                        "Click the bin again to remove".to_string()
                                    } else {
                                        w.meta()
                                    })
                                    .flex_1(),
                                )
                                .when(!w.primary, |d| {
                                    d.child(
                                        div()
                                            .id(SharedString::from(format!("del-{i}")))
                                            .invisible()
                                            .group_hover(group.clone(), |s| s.visible())
                                            .when(confirm, |d| d.visible())
                                            .cursor_pointer()
                                            .child(icon(
                                                "trash",
                                                12.,
                                                if confirm { t.err } else { t.ink_3 },
                                            ))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                if this.confirm_delete.as_ref() == Some(&path) {
                                                    this.delete_worktree(path.clone(), cx);
                                                } else {
                                                    this.confirm_delete = Some(path.clone());
                                                    cx.notify();
                                                }
                                            })),
                                    )
                                }),
                        ),
                );
            list = list.child(row);
        }
        if scanning {
            list = list.child(
                div()
                    .px(px(8.))
                    .py(px(6.))
                    .text_size(metrics::TEXT_XS)
                    .text_color(t.ink_3)
                    .child("Scanning worktrees…"),
            );
        }
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .px(px(16.))
                    .py(px(6.))
                    .text_size(metrics::TEXT_XS)
                    .text_color(t.ink_3)
                    .child("Projects"),
            )
            .child(
                div()
                    .id("wt-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .on_scroll_wheel(cx.listener(Self::on_side_wheel))
                    .child(
                        div()
                            .relative()
                            .left(px(dx))
                            .opacity(1. - dx.abs() / 140.)
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .pl(px(14.))
                                    .pr(px(12.))
                                    .pt(px(2.))
                                    .pb(px(10.))
                                    .child(
                                        div()
                                            .size(px(24.))
                                            .rounded(px(5.))
                                            .bg(t.hover_2)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(metrics::TEXT_XS)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(t.ink_2)
                                            .child(letter),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .text_size(metrics::TEXT)
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(proj_name),
                                            )
                                            .child(
                                                div()
                                                    .text_size(metrics::TEXT_XS)
                                                    .text_color(t.ink_3)
                                                    .child(format!(
                                                        "{stack} · {} worktrees",
                                                        wts.len()
                                                    )),
                                            ),
                                    )
                                    .child(
                                        ui::icon_button("new-wt", "plus", 12., t)
                                            .size(px(24.))
                                            .on_click(cx.listener(|this, _, w, cx| {
                                                this.start_new_worktree(w, cx)
                                            })),
                                    ),
                            )
                            .child(list),
                    ),
            )
            .into_any_element()
    }

    fn render_files(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(root) = self.active_wt_path() else {
            return div().into_any_element();
        };
        let mut col = div().flex().flex_col().px(px(8.)).pb(px(8.));
        let key = self.tree_open.len();
        let rows = match &self.tree_cache {
            Some((r, k, rows)) if *r == root && *k == key => rows.clone(),
            _ if insyde_core::remote::is_remote(&root) => {
                // Remote listings take a round-trip each: build off the UI thread.
                let placeholder = std::sync::Arc::new(Vec::new());
                self.tree_cache = Some((root.clone(), key, placeholder.clone()));
                let (r2, open, r3) = (root.clone(), self.tree_open.clone(), root.clone());
                let task = cx.background_spawn(async move {
                    let mut rows = Vec::new();
                    collect_tree(&r2, &r2, 0, &open, &mut rows, 1500);
                    rows
                });
                cx.spawn(async move |this, cx| {
                    let rows = task.await;
                    let _ = this.update(cx, |this, cx| {
                        if let Some((r, k, _)) = &this.tree_cache
                            && *r == r3
                            && *k == key
                        {
                            this.tree_cache = Some((r3, key, std::sync::Arc::new(rows)));
                            cx.notify();
                        }
                    });
                })
                .detach();
                placeholder
            }
            _ => {
                let mut rows = Vec::new();
                collect_tree(&root, &root, 0, &self.tree_open, &mut rows, 1500);
                let rows = std::sync::Arc::new(rows);
                self.tree_cache = Some((root.clone(), key, rows.clone()));
                rows
            }
        };
        for (i, (p, depth, is_dir)) in rows.iter().cloned().enumerate() {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let rel = p
                .strip_prefix(&root)
                .map(|r| r.to_string_lossy().into_owned())
                .unwrap_or_default();
            let open = self.tree_open.contains(&p);
            let hover = t.hover;
            let sel = self
                .wt()
                .and_then(|w| w.editor.as_ref())
                .is_some_and(|e| e.read(cx).rel == rel);
            col = col.child(
                div()
                    .id(SharedString::from(format!("f-{i}")))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(24.))
                    .pl(px(8. + depth as f32 * 14.))
                    .pr(px(8.))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .text_size(metrics::TEXT_SM)
                    .text_color(if sel { t.ink } else { t.ink_2 })
                    .bg(if sel {
                        t.sel_bg
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(move |s| s.bg(hover))
                    .child(if is_dir {
                        icon(
                            if open {
                                "chevron-down"
                            } else {
                                "chevron-right"
                            },
                            10.,
                            t.ink_4,
                        )
                    } else {
                        icon("file", 12., t.ink_3)
                    })
                    .child(ui::trunc(name))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        if is_dir {
                            if !this.tree_open.remove(&p) {
                                this.tree_open.insert(p.clone());
                            }
                            this.tree_cache = None;
                            cx.notify();
                        } else {
                            this.open_file(rel.clone(), None, w, cx);
                        }
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .px(px(16.))
                    .py(px(4.))
                    .flex()
                    .items_center()
                    .text_size(metrics::TEXT_XS)
                    .text_color(t.ink_3)
                    .child("Files")
                    .child(div().flex_1())
                    .child(
                        ui::icon_button("refresh-tree", "refresh", 11., t)
                            .size(px(20.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tree_cache = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("files")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(col),
            )
            .into_any_element()
    }

    fn render_search(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div().flex().flex_col().px(px(8.));
        for (i, (path, line, text)) in self.search_results.iter().enumerate() {
            let hover = t.hover;
            let (p, l) = (path.clone(), *line);
            list =
                list.child(
                    div()
                        .id(SharedString::from(format!("sr-{i}")))
                        .px(px(8.))
                        .py(px(5.))
                        .rounded(px(5.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .child(
                            div()
                                .flex()
                                .gap(px(6.))
                                .text_size(metrics::TEXT_XS)
                                .text_color(t.ink_3)
                                .child(ui::trunc(path.clone()))
                                .child(div().flex_none().child(format!(":{line}"))),
                        )
                        .child(
                            ui::trunc(text.clone())
                                .font_family(metrics::MONO_FONT)
                                .text_size(metrics::TEXT_MONO)
                                .text_color(t.ink_2),
                        )
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.open_file(p.clone(), Some(l), w, cx)
                        })),
                );
        }
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div().px(px(10.)).pb(px(8.)).child(
                    Input::new(&self.search)
                        .prefix(icon("search", 13., t.ink_faint))
                        .h(px(30.)),
                ),
            )
            .child(
                div()
                    .px(px(16.))
                    .pb(px(4.))
                    .text_size(metrics::TEXT_XS)
                    .text_color(t.ink_3)
                    .child(if self.search_results.is_empty() {
                        "Type to search file contents".to_string()
                    } else {
                        format!("{} matches", self.search_results.len())
                    }),
            )
            .child(
                div()
                    .id("sr")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .into_any_element()
    }
}

/// Visible rows of the lazily expanded file tree (respects .gitignore).
fn collect_tree(
    root: &std::path::Path,
    dir: &std::path::Path,
    depth: usize,
    open: &std::collections::HashSet<PathBuf>,
    out: &mut Vec<(PathBuf, usize, bool)>,
    cap: usize,
) {
    if out.len() >= cap || depth > 12 {
        return;
    }
    if insyde_core::remote::is_remote(dir) {
        for (name, is_dir) in insyde_core::remote::list_dir(dir) {
            if out.len() >= cap {
                return;
            }
            let p = dir.join(&name);
            out.push((p.clone(), depth, is_dir));
            if is_dir && open.contains(&p) {
                collect_tree(root, &p, depth + 1, open, out, cap);
            }
        }
        return;
    }
    let walker = ignore::WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(true)
        .git_ignore(true)
        .parents(true)
        .build();
    let mut entries: Vec<(PathBuf, bool)> = walker
        .filter_map(|e| e.ok())
        .filter(|e| e.depth() == 1)
        .map(|e| {
            (
                e.path().to_path_buf(),
                e.file_type().is_some_and(|t| t.is_dir()),
            )
        })
        .collect();
    entries.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.file_name().cmp(&b.0.file_name()))
    });
    let _ = root;
    for (p, is_dir) in entries {
        if out.len() >= cap {
            return;
        }
        out.push((p.clone(), depth, is_dir));
        if is_dir && open.contains(&p) {
            collect_tree(root, &p, depth + 1, open, out, cap);
        }
    }
}
