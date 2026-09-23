//! Project Brain screen (full-window overlay): area tree, interactive graph,
//! and node inspector — plus a Notes mode for SiYuan-style linked notes.

use crate::ui::{self, icon};
use gpui::prelude::*;
use gpui::{
    Bounds, Context, Entity, EventEmitter, FontWeight, Hsla, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, Point, ScrollDelta, ScrollWheelEvent, SharedString, Subscription,
    Window, canvas, div, fill, point, px, size,
};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use insyde_core::brain::layout::Layout;
use insyde_core::brain::{Brain, EdgeKind, Graph, Kind};
use insyde_theme::{ActiveTheme, Theme, metrics};
use parking_lot::RwLock;
use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

/// Shared, thread-safe access to one project's brain.
pub struct BrainHandle {
    pub brain: Brain,
    pub graph: RwLock<Graph>,
}

impl BrainHandle {
    pub fn node_count(&self) -> usize {
        self.graph.read().nodes.len()
    }
    pub fn total_tokens(&self) -> u64 {
        self.graph.read().total_tokens()
    }
    pub fn digest(&self, project: &str, task: &str) -> String {
        let g = self.graph.read();
        let budget = insyde_core::settings::get()
            .brain_budget_tokens
            .clamp(2_000, 100_000) as usize;
        self.brain.digest(&g, project, task, budget)
    }
}

pub fn kind_color(k: Kind, t: &Theme) -> Hsla {
    let p = &t.palette;
    match k {
        Kind::Root => t.ink,
        Kind::Module => p.blue,
        Kind::File => p.teal,
        Kind::Symbol => p.gray,
        Kind::Decision => p.red,
        Kind::Convention => p.green,
        Kind::Api => p.amber,
        Kind::Pr => p.purple,
        Kind::Doc => p.orange,
    }
}

fn radius(k: Kind) -> f32 {
    match k {
        Kind::Root => 10.,
        Kind::Module => 7.,
        Kind::Symbol => 2.6,
        _ => 4.2,
    }
}

pub enum BrainEvent {
    Close,
    Update,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Graph,
    Notes,
}

struct Drag {
    start: Point<Pixels>,
    node: Option<usize>,
    pan0: (f32, f32),
    moved: f32,
}

pub struct BrainView {
    pub handle: Arc<BrainHandle>,
    project: String,
    updated: String,
    layout: Layout,
    nb: Vec<Vec<usize>>,
    sel: usize,
    hover: Option<usize>,
    zoom: f32,
    pan: (f32, f32),
    alpha: f32,
    drag: Option<Drag>,
    open_groups: HashSet<usize>,
    mode: Mode,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    search: Entity<InputState>,
    results: Vec<usize>,
    note: Entity<TextareaState>,
    note_for: Option<usize>,
    fitted: bool,
    _subs: Vec<Subscription>,
}

impl EventEmitter<BrainEvent> for BrainView {}

impl BrainView {
    pub fn new(
        handle: Arc<BrainHandle>,
        project: String,
        updated: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (layout, nb, sel) = {
            let g = handle.graph.read();
            let mut l = Layout::new(&g);
            l.settle(if g.nodes.len() > 1500 { 120 } else { 260 });
            let sel = g
                .nodes
                .iter()
                .position(|n| n.kind == Kind::Module)
                .unwrap_or(0);
            (l, g.neighbors(), sel)
        };
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search context"));
        let note = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(6, 18)
                .placeholder("Write a note. Link other nodes with [[Name]].")
        });
        let subs = vec![
            cx.subscribe_in(&search, window, |this, s, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Change | InputEvent::PressEnter { .. }) {
                    let q = s.read(cx).value().to_string();
                    this.results = if q.trim().is_empty() {
                        vec![]
                    } else {
                        this.handle.brain.search(&q, 30)
                    };
                    if let (InputEvent::PressEnter { .. }, Some(&first)) =
                        (ev, this.results.first())
                    {
                        this.select(first, true, cx);
                    }
                    cx.notify();
                }
            }),
        ];
        let mut open_groups = HashSet::new();
        open_groups.insert(0);
        Self {
            handle,
            project,
            updated,
            layout,
            nb,
            sel,
            hover: None,
            zoom: 1.,
            pan: (0., 0.),
            alpha: 0.15,
            drag: None,
            open_groups,
            mode: Mode::Graph,
            bounds: Rc::new(Cell::new(Bounds::default())),
            search,
            results: vec![],
            note,
            note_for: None,
            fitted: false,
            _subs: subs,
        }
    }

    fn fit(&mut self) {
        let b = self.bounds.get();
        let (x0, y0, x1, y1) = self.layout.bounds();
        let (w, h) = (
            f32::from(b.size.width).max(1.),
            f32::from(b.size.height).max(1.),
        );
        let z = (w / (x1 - x0 + 160.))
            .min(h / (y1 - y0 + 160.))
            .clamp(0.2, 2.4);
        self.zoom = z;
        self.pan = (-(x0 + x1) / 2. * z, -(y0 + y1) / 2. * z);
    }

    fn zoom_by(&mut self, f: f32) {
        let z = (self.zoom * f).clamp(0.3, 4.);
        self.pan.0 *= z / self.zoom;
        self.pan.1 *= z / self.zoom;
        self.zoom = z;
    }

    fn select(&mut self, id: usize, center: bool, cx: &mut Context<Self>) {
        self.sel = id;
        if center && let Some(&(x, y)) = self.layout.pos.get(id) {
            self.pan = (-x * self.zoom, -y * self.zoom);
        }
        if let Some(g) = self.handle.graph.read().nodes.get(id).and_then(|n| n.group) {
            self.open_groups.insert(g);
        }
        cx.notify();
    }

    fn to_world(&self, p: Point<Pixels>) -> (f32, f32) {
        let b = self.bounds.get();
        let cx = f32::from(b.origin.x) + f32::from(b.size.width) / 2.;
        let cy = f32::from(b.origin.y) + f32::from(b.size.height) / 2.;
        (
            (f32::from(p.x) - cx - self.pan.0) / self.zoom,
            (f32::from(p.y) - cy - self.pan.1) / self.zoom,
        )
    }

    fn hit(&self, p: Point<Pixels>) -> Option<usize> {
        let (x, y) = self.to_world(p);
        let g = self.handle.graph.read();
        let mut best = None;
        let mut bd = f32::MAX;
        for (i, &(nx, ny)) in self.layout.pos.iter().enumerate() {
            let d = (nx - x).powi(2) + (ny - y).powi(2);
            let r = radius(g.nodes[i].kind) + 5. / self.zoom;
            if d < r * r && d < bd {
                bd = d;
                best = Some(i);
            }
        }
        best
    }

    fn on_down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let node = self.hit(e.position);
        if node.is_some() {
            self.alpha = 0.4;
        }
        self.drag = Some(Drag {
            start: e.position,
            node,
            pan0: self.pan,
            moved: 0.,
        });
        cx.notify();
    }

    fn on_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = &mut self.drag {
            if !e.dragging() {
                self.drag = None;
                return;
            }
            let dx = f32::from(e.position.x - d.start.x);
            let dy = f32::from(e.position.y - d.start.y);
            d.moved = d.moved.max(dx.abs() + dy.abs());
            match d.node {
                Some(n) => {
                    let w = self.to_world(e.position);
                    self.layout.pos[n] = w;
                    self.alpha = 0.4;
                }
                None => self.pan = (d.pan0.0 + dx, d.pan0.1 + dy),
            }
            cx.notify();
            return;
        }
        let h = self.hit(e.position);
        if h != self.hover {
            self.hover = h;
            cx.notify();
        }
    }

    fn on_up(&mut self, cx: &mut Context<Self>) {
        if let Some(d) = self.drag.take()
            && let (Some(n), true) = (d.node, d.moved < 4.)
        {
            self.select(n, false, cx);
        }
        cx.notify();
    }

    fn on_wheel(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let dy = match e.delta {
            ScrollDelta::Pixels(p) => f32::from(p.y),
            ScrollDelta::Lines(l) => l.y * 40.,
        };
        let b = self.bounds.get();
        let mx = f32::from(e.position.x - b.origin.x) - f32::from(b.size.width) / 2.;
        let my = f32::from(e.position.y - b.origin.y) - f32::from(b.size.height) / 2.;
        let z = (self.zoom * (dy * 0.0015).exp()).clamp(0.3, 4.);
        self.pan.0 = mx - (mx - self.pan.0) * z / self.zoom;
        self.pan.1 = my - (my - self.pan.1) * z / self.zoom;
        self.zoom = z;
        cx.notify();
    }

    fn save_note(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.note_for else { return };
        let text = self.note.read(cx).value().to_string();
        let targets = {
            let g = self.handle.graph.read();
            self.handle.brain.set_note(id, &text, &g)
        };
        {
            let mut g = self.handle.graph.write();
            if let Some(n) = g.nodes.get_mut(id) {
                n.note = text;
            }
            for t in targets {
                g.edges.push((id, t, EdgeKind::Cross));
            }
            self.nb = g.neighbors();
        }
        cx.notify();
    }

    fn render_graph(
        &mut self,
        t: &Theme,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if self.alpha > 0.01 {
            let pinned = self.drag.as_ref().and_then(|d| d.node);
            self.layout.tick(self.alpha, pinned);
            self.alpha *= 0.97;
            window.request_animation_frame();
        }
        let g = self.handle.graph.read();
        let focus = self.hover.unwrap_or(self.sel);
        let fset: HashSet<usize> = self
            .nb
            .get(focus)
            .map(|v| v.iter().copied().collect())
            .unwrap_or_default();
        let pos = self.layout.pos.clone();
        let kinds: Vec<Kind> = g.nodes.iter().map(|n| n.kind).collect();
        let names: Vec<SharedString> = g
            .nodes
            .iter()
            .map(|n| SharedString::from(n.name.clone()))
            .collect();
        let edges: Vec<(usize, usize, EdgeKind)> = g.edges.clone();
        drop(g);
        let (zoom, pan, sel) = (self.zoom, self.pan, self.sel);
        let bounds_cell = self.bounds.clone();
        let t = t.clone();
        canvas(
            move |b, _, _| {
                bounds_cell.set(b);
            },
            move |b, _, window, cx| {
                let c = point(
                    b.origin.x + b.size.width / 2. + px(pan.0),
                    b.origin.y + b.size.height / 2. + px(pan.1),
                );
                let to = |(x, y): (f32, f32)| point(c.x + px(x * zoom), c.y + px(y * zoom));
                window.with_content_mask(Some(gpui::ContentMask { bounds: b }), |window| {
                    // Edges: two batched paths (dim, highlighted).
                    let mut dim = gpui::PathBuilder::stroke(px(0.8));
                    let mut hot = gpui::PathBuilder::stroke(px(1.4));
                    let (mut n_dim, mut n_hot) = (0, 0);
                    for &(a, bb, _) in &edges {
                        if a >= pos.len() || bb >= pos.len() {
                            continue;
                        }
                        let on = a == focus || bb == focus;
                        let pb = if on { &mut hot } else { &mut dim };
                        pb.move_to(to(pos[a]));
                        pb.line_to(to(pos[bb]));
                        if on { n_hot += 1 } else { n_dim += 1 }
                    }
                    if n_dim > 0
                        && let Ok(p) = dim.build()
                    {
                        window.paint_path(p, t.ink_3.opacity(0.14));
                    }
                    if n_hot > 0
                        && let Ok(p) = hot.build()
                    {
                        window.paint_path(p, t.accent.opacity(0.9));
                    }
                    // Nodes.
                    for (i, &p) in pos.iter().enumerate() {
                        let near = i == focus || fset.contains(&i);
                        let r = radius(kinds[i]) * zoom.max(0.6);
                        let mut col = kind_color(kinds[i], &t);
                        if !near {
                            col = col.opacity(0.28);
                        }
                        let q = to(p);
                        window.paint_quad(
                            fill(
                                Bounds::new(
                                    point(q.x - px(r), q.y - px(r)),
                                    size(px(2. * r), px(2. * r)),
                                ),
                                col,
                            )
                            .corner_radii(px(r)),
                        );
                        if i == sel {
                            let rr = r + 3.5;
                            window.paint_quad(
                                gpui::outline(
                                    Bounds::new(
                                        point(q.x - px(rr), q.y - px(rr)),
                                        size(px(2. * rr), px(2. * rr)),
                                    ),
                                    t.accent,
                                    gpui::BorderStyle::Solid,
                                )
                                .corner_radii(px(rr))
                                .border_widths(px(2.)),
                            );
                        }
                    }
                    // Labels: hubs always; focus neighborhood; everything else when zoomed in.
                    for (i, &p) in pos.iter().enumerate() {
                        let k = kinds[i];
                        let big = matches!(k, Kind::Root | Kind::Module);
                        let in_f = i == focus || fset.contains(&i);
                        if !(big || in_f || (zoom > 1.7 && k != Kind::Symbol) || zoom > 2.8) {
                            continue;
                        }
                        let fs = px(if big { 12. } else { 11. });
                        let mut f = gpui::font(metrics::UI_FONT);
                        if big {
                            f.weight = FontWeight::SEMIBOLD;
                        }
                        let color = if big { t.ink } else { t.ink_2 };
                        let dimmed = !fset.is_empty() && !in_f && !big;
                        let color = if dimmed { color.opacity(0.4) } else { color };
                        let text = names[i].clone();
                        let run = gpui::TextRun {
                            len: text.len(),
                            font: f,
                            color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let line = window.text_system().shape_line(text, fs, &[run], None);
                        let q = to(p);
                        let r = radius(k) * zoom.max(0.6);
                        let origin = point(q.x - line.width / 2., q.y + px(r + 3.));
                        let _ =
                            line.paint(origin, px(14.), gpui::TextAlign::Left, None, window, cx);
                    }
                });
            },
        )
        .size_full()
    }

    fn render_inspector(&mut self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let g = self.handle.graph.read();
        let Some(n) = g.nodes.get(self.sel).cloned() else {
            return div();
        };
        let group = n
            .group
            .and_then(|gi| g.groups.get(gi).cloned())
            .unwrap_or_else(|| "root".into());
        let links: Vec<(usize, String, Kind)> = self
            .nb
            .get(n.id)
            .map(|v| {
                v.iter()
                    .filter_map(|&i| g.nodes.get(i).map(|x| (i, x.name.clone(), x.kind)))
                    .collect()
            })
            .unwrap_or_default();
        let sha = g.sha.clone();
        drop(g);
        let mut linked = div().flex().flex_col().gap(px(1.));
        for (i, name, k) in links.iter().take(8).cloned() {
            let h = t.hover;
            linked = linked.child(
                div()
                    .id(SharedString::from(format!("ln-{i}")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(28.))
                    .px(px(6.))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_2)
                    .hover(move |s| s.bg(h))
                    .child(ui::dot(kind_color(k, t), 7.))
                    .child(ui::trunc(name).flex_1())
                    .child(
                        div()
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child(k.label()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select(i, true, cx))),
            );
        }
        let field = |label: &str, value: String, t: &Theme| {
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(metrics::TEXT_XS)
                        .text_color(t.ink_3)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(28.))
                        .px(px(8.))
                        .bg(t.panel_2)
                        .border_1()
                        .border_color(t.field_border)
                        .rounded(metrics::RADIUS)
                        .text_size(metrics::TEXT_SM)
                        .text_color(t.ink)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(value),
                )
        };
        let changed = n
            .changed
            .map(insyde_core::git::ago)
            .unwrap_or_else(|| "—".into());
        div()
            .flex()
            .flex_col()
            .min_h_0()
            .h_full()
            .bg(t.panel)
            .border_l_1()
            .border_color(t.line)
            .overflow_hidden()
            .child(
                div()
                    .px(px(14.))
                    .pt(px(14.))
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
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(10.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(t.palette.white)
                                    .bg(if n.kind == Kind::Root {
                                        t.palette.slate
                                    } else {
                                        kind_color(n.kind, t)
                                    })
                                    .child(n.kind.chip()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        ui::trunc(n.name.clone())
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_size(metrics::TEXT)
                                            .text_color(t.ink),
                                    )
                                    .child(
                                        div()
                                            .mt(px(1.))
                                            .text_size(metrics::TEXT_XS)
                                            .text_color(t.ink_3)
                                            .child(format!("{} · {}", n.kind.label(), group)),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .id("insp-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .px(px(14.))
                            .pt(px(12.))
                            .pb(px(14.))
                            .border_b_1()
                            .border_color(t.line)
                            .child(
                                div()
                                    .text_size(metrics::TEXT_SM)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .mb(px(8.))
                                    .text_color(t.ink)
                                    .child("Summary"),
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(8.))
                                    .bg(t.panel_2)
                                    .border_1()
                                    .border_color(t.field_border)
                                    .rounded(metrics::RADIUS)
                                    .text_size(metrics::TEXT_SM)
                                    .text_color(t.ink_2)
                                    .child(n.summary.clone()),
                            )
                            .when(!n.note.is_empty(), |d| {
                                d.child(
                                    div()
                                        .mt(px(8.))
                                        .text_size(metrics::TEXT_XS)
                                        .text_color(t.ink_3)
                                        .child(format!("Note: {}", n.note)),
                                )
                            }),
                    )
                    .child(
                        div()
                            .px(px(14.))
                            .pt(px(12.))
                            .pb(px(14.))
                            .border_b_1()
                            .border_color(t.line)
                            .child(
                                div()
                                    .flex()
                                    .mb(px(6.))
                                    .text_size(metrics::TEXT_SM)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(t.ink)
                                    .child("Linked")
                                    .child(
                                        div()
                                            .ml_auto()
                                            .font_weight(FontWeight::NORMAL)
                                            .text_size(metrics::TEXT_XS)
                                            .text_color(t.ink_3)
                                            .child(links.len().to_string()),
                                    ),
                            )
                            .child(linked),
                    )
                    .child(
                        div()
                            .px(px(14.))
                            .pt(px(12.))
                            .pb(px(14.))
                            .child(
                                div()
                                    .text_size(metrics::TEXT_SM)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .mb(px(8.))
                                    .text_color(t.ink)
                                    .child("Details"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(8.))
                                    .child(field("Source", format!("main @ {sha}"), t).w(px(130.)))
                                    .child(
                                        field(
                                            "Size",
                                            format!("{:.1}k tokens", n.tokens as f32 / 1000.),
                                            t,
                                        )
                                        .w(px(130.)),
                                    )
                                    .child(field("Last changed", changed, t).w(px(130.)))
                                    .child(
                                        field("Used by agents", format!("{} times", n.uses), t)
                                            .w(px(130.)),
                                    ),
                            ),
                    ),
            )
            .child(
                div().p(px(10.)).border_t_1().border_color(t.line).child(
                    ui::button("pin", t)
                        .w_full()
                        .h(px(32.))
                        .px(px(10.))
                        .gap(px(10.))
                        .text_size(metrics::TEXT_SM)
                        .child(div().flex_1().child("Always include in agent context"))
                        .child(ui::switch(n.pinned, t))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let id = this.sel;
                            let v = {
                                let mut g = this.handle.graph.write();
                                let node = &mut g.nodes[id];
                                node.pinned = !node.pinned;
                                node.pinned
                            };
                            this.handle.brain.set_pinned(id, v);
                            cx.notify();
                        })),
                ),
            )
    }
}

impl Render for BrainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        if !self.fitted && f32::from(self.bounds.get().size.width) > 0. {
            self.fit();
            self.fitted = true;
        }
        let (n_nodes, n_edges, sha, groups) = {
            let g = self.handle.graph.read();
            (
                g.nodes.len(),
                g.edges.len(),
                g.sha.clone(),
                g.groups.clone(),
            )
        };

        // Header.
        let header = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(metrics::TOPBAR_H)
            .pl(px(84.))
            .pr(px(12.))
            .bg(t.panel)
            .border_b_1()
            .border_color(t.line)
            .child(
                ui::icon_button("brain-back", "chevron-left", 14., &t)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(BrainEvent::Close))),
            )
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.))
                    .pr(px(14.))
                    .mr(px(4.))
                    .border_r_1()
                    .border_color(t.line)
                    .child(
                        div()
                            .text_size(metrics::TEXT_TITLE)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.ink)
                            .child("Project Brain"),
                    )
                    .child(
                        div()
                            .text_size(metrics::TEXT_SM)
                            .text_color(t.ink_3)
                            .child(self.project.clone()),
                    ),
            )
            .child(
                div()
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink_3)
                    .child(format!(
                        "Built from main @ {sha} · {n_nodes} notes · {n_edges} links · {}",
                        self.updated
                    )),
            )
            .child(div().flex_1())
            .child(
                div().w(px(220.)).child(
                    Input::new(&self.search)
                        .prefix(icon("search", 13., t.ink_faint))
                        .h(px(30.)),
                ),
            )
            .child(ui::segmented(
                "brain-mode",
                &["Graph", "Notes"],
                if self.mode == Mode::Graph { 0 } else { 1 },
                &t,
                false,
                26.,
                {
                    let e = cx.entity().downgrade();
                    move |i, _, cx| {
                        let _ = e.update(cx, |this, cx| {
                            this.mode = if i == 0 { Mode::Graph } else { Mode::Notes };
                            cx.notify();
                        });
                    }
                },
            ))
            .child(
                ui::button("brain-update", &t)
                    .child(icon("refresh", 13., t.ink))
                    .child("Update")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(BrainEvent::Update))),
            )
            .child(
                ui::primary_button("brain-done", &t)
                    .child("Done")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(BrainEvent::Close))),
            );

        // Left: areas tree (or search results).
        let mut tree = div()
            .id("brain-tree")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(8.))
            .pb(px(12.));
        {
            let g = self.handle.graph.read();
            if !self.results.is_empty() {
                for &id in &self.results {
                    if let Some(n) = g.nodes.get(id) {
                        tree = tree.child(tree_item(id, n.kind, &n.name, id == self.sel, &t, cx));
                    }
                }
            } else {
                for (gi, name) in groups.iter().enumerate() {
                    let items: Vec<(usize, Kind, String)> = g
                        .nodes
                        .iter()
                        .filter(|n| {
                            n.group == Some(gi) && !matches!(n.kind, Kind::Symbol | Kind::Module)
                        })
                        .map(|n| (n.id, n.kind, n.name.clone()))
                        .collect();
                    let open = self.open_groups.contains(&gi);
                    let h = t.hover;
                    let mut block = div().mb(px(2.)).child(
                        div()
                            .id(SharedString::from(format!("grp-{gi}")))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .h(px(30.))
                            .px(px(8.))
                            .rounded(metrics::RADIUS)
                            .cursor_pointer()
                            .hover(move |s| s.bg(h))
                            .child(icon(
                                if open {
                                    "chevron-down"
                                } else {
                                    "chevron-right"
                                },
                                10.,
                                t.ink_4,
                            ))
                            .child(div().size(px(8.)).rounded(px(2.)).bg(t.palette.blue))
                            .child(
                                ui::trunc(name.clone())
                                    .flex_1()
                                    .text_size(metrics::TEXT_SM)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(t.ink),
                            )
                            .child(
                                div()
                                    .text_size(metrics::TEXT_XS)
                                    .text_color(t.ink_3)
                                    .child(items.len().to_string()),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.open_groups.remove(&gi) {
                                    this.open_groups.insert(gi);
                                }
                                cx.notify();
                            })),
                    );
                    if open {
                        let mut list = div()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .pl(px(18.))
                            .pt(px(1.))
                            .pb(px(4.));
                        for (id, k, name) in items.into_iter().take(200) {
                            list = list.child(tree_item(id, k, &name, id == self.sel, &t, cx));
                        }
                        block = block.child(list);
                    }
                    tree = tree.child(block);
                }
            }
        }
        let left = div()
            .flex()
            .flex_col()
            .min_h_0()
            .h_full()
            .bg(t.panel)
            .border_r_1()
            .border_color(t.line)
            .overflow_hidden()
            .child(
                div()
                    .px(px(16.))
                    .pt(px(12.))
                    .pb(px(8.))
                    .flex()
                    .items_center()
                    .text_size(metrics::TEXT_XS)
                    .text_color(t.ink_3)
                    .child(if self.results.is_empty() { "Context".to_string() } else { format!("{} matches", self.results.len()) })
                    .child(div().ml_auto().child(format!("{} areas", groups.len()))),
            )
            .child(tree)
            .child(
                div()
                    .px(px(14.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(t.line)
                    .text_size(metrics::TEXT_XS)
                    .text_color(t.ink_3)
                    .child(format!("New worktrees in {} get this brain by default. Turn it off per agent in the composer.", self.project)),
            );

        // Center.
        let center = match self.mode {
            Mode::Graph => {
                let mut legend = div()
                    .absolute()
                    .left(px(12.))
                    .top(px(12.))
                    .flex()
                    .flex_wrap()
                    .gap_x(px(12.))
                    .gap_y(px(4.))
                    .max_w(px(420.))
                    .px(px(10.))
                    .py(px(8.))
                    .bg(t.panel)
                    .border_1()
                    .border_color(t.line)
                    .rounded(px(7.))
                    .shadow(t.pop_shadow(false));
                for k in Kind::ALL {
                    legend = legend.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_2)
                            .child(ui::dot(kind_color(k, &t), 8.))
                            .child(k.label()),
                    );
                }
                let zoom_label = format!("{}%", (self.zoom * 100.).round());
                div()
                    .id("graph")
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .bg(t.ground)
                    .cursor(if self.hover.is_some() {
                        gpui::CursorStyle::PointingHand
                    } else if self.drag.is_some() {
                        gpui::CursorStyle::ClosedHand
                    } else {
                        gpui::CursorStyle::OpenHand
                    })
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
                    .on_mouse_move(cx.listener(Self::on_move))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.on_up(cx)),
                    )
                    .on_scroll_wheel(cx.listener(Self::on_wheel))
                    .child(self.render_graph(&t, window, cx))
                    .child(legend)
                    .child(
                        div()
                            .absolute()
                            .bottom(px(14.))
                            .left_1_2()
                            .ml(px(-90.))
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .p(px(3.))
                            .bg(t.panel)
                            .border_1()
                            .border_color(t.field_border)
                            .rounded(px(7.))
                            .shadow(t.pop_shadow(false))
                            .occlude()
                            .child(
                                ui::icon_button("zoom-out", "minus", 12., &t)
                                    .w(px(28.))
                                    .h(px(26.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.zoom_by(0.8);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .min_w(px(44.))
                                    .text_center()
                                    .text_size(metrics::TEXT_XS)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(t.ink_2)
                                    .child(zoom_label),
                            )
                            .child(
                                ui::icon_button("zoom-in", "plus", 12., &t)
                                    .w(px(28.))
                                    .h(px(26.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.zoom_by(1.25);
                                        cx.notify();
                                    })),
                            )
                            .child(div().w(px(1.)).h(px(16.)).mx(px(4.)).bg(t.line))
                            .child(
                                div()
                                    .id("fit")
                                    .h(px(26.))
                                    .px(px(10.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(5.))
                                    .cursor_pointer()
                                    .text_size(metrics::TEXT_SM)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(t.ink_2)
                                    .child("Fit")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.fit();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(px(12.))
                            .bottom(px(14.))
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_faint)
                            .child("Drag to pan · scroll to zoom · click a node"),
                    )
                    .into_any_element()
            }
            Mode::Notes => {
                if self.note_for != Some(self.sel) {
                    let text = self
                        .handle
                        .graph
                        .read()
                        .nodes
                        .get(self.sel)
                        .map(|n| n.note.clone())
                        .unwrap_or_default();
                    self.note.update(cx, |s, cx| s.set_value(text, window, cx));
                    self.note_for = Some(self.sel);
                }
                let (name, kind, summary) = {
                    let g = self.handle.graph.read();
                    g.nodes
                        .get(self.sel)
                        .map(|n| (n.name.clone(), n.kind, n.summary.clone()))
                        .unwrap_or((String::new(), Kind::Doc, String::new()))
                };
                div()
                    .id("notes")
                    .size_full()
                    .overflow_y_scroll()
                    .bg(t.ground)
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(720.))
                            .px(px(28.))
                            .py(px(24.))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(div().flex().items_center().gap(px(8.)).child(ui::dot(kind_color(kind, &t), 8.)).child(div().text_size(metrics::TEXT_XS).text_color(t.ink_3).child(kind.label())))
                            .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(t.ink).child(name))
                            .child(div().text_size(metrics::TEXT_SM).text_color(t.ink_2).child(summary))
                            .child(div().mt(px(8.)).text_size(metrics::TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(t.ink).child("Your note"))
                            .child(div().bg(t.panel).border_1().border_color(t.field_border).rounded(px(8.)).p(px(6.)).child(Textarea::new(&self.note).appearance(false)))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(div().flex_1().text_size(metrics::TEXT_XS).text_color(t.ink_3).child("Notes are searchable and ride along whenever this node is in an agent's context. [[Links]] add graph edges."))
                                    .child(ui::primary_button("save-note", &t).h(px(28.)).text_size(metrics::TEXT_SM).child("Save note").on_click(cx.listener(|this, _, _, cx| this.save_note(cx)))),
                            ),
                    )
                    .into_any_element()
            }
        };

        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .bg(t.ground)
            .occlude()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(div().w(px(248.)).flex_none().h_full().child(left))
                    .child(div().flex_1().min_w_0().h_full().child(center))
                    .child(
                        div()
                            .w(px(300.))
                            .flex_none()
                            .h_full()
                            .child(self.render_inspector(&t, cx)),
                    ),
            )
    }
}

fn tree_item(
    id: usize,
    k: Kind,
    name: &str,
    on: bool,
    t: &Theme,
    cx: &mut Context<BrainView>,
) -> impl IntoElement {
    let h = t.hover;
    div()
        .id(SharedString::from(format!("ti-{id}")))
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(28.))
        .px(px(8.))
        .rounded(px(5.))
        .cursor_pointer()
        .text_size(metrics::TEXT_SM)
        .bg(if on {
            t.sel_bg
        } else {
            gpui::transparent_black()
        })
        .text_color(if on { t.ink } else { t.ink_2 })
        .hover(move |s| s.bg(h))
        .child(
            div()
                .flex_none()
                .min_w(px(22.))
                .h(px(18.))
                .px(px(4.))
                .rounded(px(3.))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .bg(if on { t.sel_chip } else { t.hover_2 })
                .text_color(if on { t.sel_text } else { t.ink_3 })
                .child(k.chip()),
        )
        .child(ui::trunc(name.to_string()).flex_1())
        .on_click(cx.listener(move |this, _, _, cx| this.select(id, true, cx)))
}
