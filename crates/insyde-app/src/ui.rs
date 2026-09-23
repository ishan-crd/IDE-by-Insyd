//! Small building blocks shared by every view. Each mirrors a recurring
//! element of the design (outlined button, primary button, segmented control,
//! agent monogram, status dot, switch) so views stay declarative.

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, AnyElement, App, Div, ElementId, FontWeight, Hsla, SharedString,
    Stateful, Svg, div, px, svg,
};
use insyde_core::agents::{AgentSpec, Tint};
use insyde_theme::{Theme, metrics};
use std::time::Duration;

pub fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

/// Outlined 30px control (`background:var(--panel);border:1px solid var(--field-border)`).
pub fn button(id: impl Into<ElementId>, t: &Theme) -> Stateful<Div> {
    let hover = t.hover;
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(7.))
        .h(metrics::CONTROL_H)
        .px(px(12.))
        .bg(t.panel)
        .border_1()
        .border_color(t.field_border)
        .rounded(metrics::RADIUS)
        .cursor_pointer()
        .text_color(t.ink)
        .font_weight(FontWeight::MEDIUM)
        .hover(move |s| s.bg(hover))
}

/// Smaller outlined control (26/28px) used in panels.
pub fn small_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    t: &Theme,
) -> Stateful<Div> {
    button(id, t)
        .h(px(26.))
        .px(px(10.))
        .text_size(metrics::TEXT_SM)
        .child(label.into())
}

pub fn primary_button(id: impl Into<ElementId>, t: &Theme) -> Stateful<Div> {
    let hover = t.primary_hover;
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(7.))
        .h(metrics::CONTROL_H)
        .px(px(14.))
        .bg(t.primary)
        .border_1()
        .border_color(t.primary)
        .rounded(metrics::RADIUS)
        .cursor_pointer()
        .text_color(t.on_primary)
        .font_weight(FontWeight::MEDIUM)
        .hover(move |s| s.bg(hover))
}

/// Borderless square icon button (28px); the glyph brightens on hover.
pub fn icon_button(id: impl Into<ElementId>, name: &str, size: f32, t: &Theme) -> Stateful<Div> {
    let (h, ink) = (t.hover_2, t.ink);
    let group = SharedString::from(format!("ib-{name}"));
    div()
        .id(id)
        .group(group.clone())
        .size(px(28.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(metrics::RADIUS)
        .cursor_pointer()
        .hover(move |s| s.bg(h))
        .child(icon(name, size, t.ink_3).group_hover(group, move |s| s.text_color(ink)))
}

/// Segmented control. `on` is called with the clicked index.
pub fn segmented(
    id: &str,
    items: &[&str],
    active: usize,
    t: &Theme,
    grow: bool,
    height: f32,
    on: impl Fn(usize, &mut gpui::Window, &mut App) + 'static,
) -> Div {
    let on = std::rc::Rc::new(on);
    let mut row = div()
        .flex()
        .gap(px(2.))
        .p(px(2.))
        .bg(t.hover_2)
        .rounded(px(7.));
    for (i, label) in items.iter().enumerate() {
        let on = on.clone();
        let is = i == active;
        let mut b = div()
            .id(SharedString::from(format!("{id}-{i}")))
            .h(px(height))
            .px(px(10.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(5.))
            .cursor_pointer()
            .text_size(metrics::TEXT_SM)
            .font_weight(FontWeight::MEDIUM)
            .child(label.to_string())
            .on_click(move |_, w, cx| on(i, w, cx));
        if grow {
            b = b.flex_1();
        }
        b = if is {
            b.bg(t.seg_active).text_color(t.ink).shadow(t.seg_shadow())
        } else {
            b.text_color(t.ink_3)
        };
        row = row.child(b);
    }
    row
}

pub fn tint_color(tint: Tint, t: &Theme) -> Hsla {
    let p = &t.palette;
    match tint {
        Tint::Outline => gpui::transparent_black(),
        Tint::Blue => p.blue,
        Tint::Orange => p.orange,
        Tint::Slate => p.slate,
        Tint::Amber => p.amber,
        Tint::Purple => p.purple,
        Tint::Graphite => p.graphite,
        Tint::Stone => p.stone,
        Tint::Teal => p.teal,
        Tint::Tool => t.hover_2,
    }
}

/// Circular agent monogram ("CC", "Cx"…).
pub fn monogram(spec: &AgentSpec, size: f32, t: &Theme) -> Div {
    let font = match size as i32 {
        0..=18 => 9.5,
        19..=22 => 10.,
        _ => 14.,
    };
    let (bg, fg) = match spec.tint {
        Tint::Outline => (gpui::transparent_black(), t.ink),
        Tint::Tool => (t.hover_2, t.ink_2),
        other => (tint_color(other, t), t.palette.white),
    };
    let mut d = div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(bg)
        .text_color(fg)
        .text_size(px(font))
        .font_weight(FontWeight::BOLD)
        .child(spec.mono);
    if spec.tint == Tint::Outline {
        d = d.border(px(1.5)).border_color(t.ink_3);
    }
    d
}

pub fn dot(color: Hsla, size: f32) -> Div {
    div().size(px(size)).flex_none().rounded_full().bg(color)
}

/// The design's `pulse` keyframes (opacity 1 → .35 → 1 over 1.4s).
pub fn pulse(id: impl Into<ElementId>, el: Div) -> AnyElement {
    el.with_animation(
        id,
        Animation::new(Duration::from_millis(1400)).repeat(),
        |el, d| {
            let x = (d * std::f32::consts::TAU).cos() * 0.5 + 0.5; // 1 → 0 → 1
            el.opacity(0.35 + 0.65 * x)
        },
    )
    .into_any_element()
}

/// iOS-style switch (28×16).
pub fn switch(on: bool, t: &Theme) -> Div {
    div()
        .relative()
        .w(px(28.))
        .h(px(16.))
        .flex_none()
        .rounded(px(16.))
        .bg(if on { t.accent } else { t.field_border })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 14. } else { 2. }))
                .size(px(12.))
                .rounded_full()
                .bg(t.palette.white),
        )
}

/// Small check box used in the hand-off popover.
pub fn checkbox(on: bool, t: &Theme) -> Div {
    let mut d = div()
        .size(px(16.))
        .flex_none()
        .rounded(px(4.))
        .flex()
        .items_center()
        .justify_center();
    if on {
        d = d.bg(t.accent).child(icon("check", 10., t.palette.white));
    } else {
        d = d.border(px(1.5)).border_color(t.field_border);
    }
    d
}

pub fn mono_text(t: &Theme) -> Div {
    div()
        .font_family(metrics::MONO_FONT)
        .text_size(metrics::TEXT_MONO)
        .text_color(t.ink_2)
}

/// Single-line text that ellipsizes.
pub fn trunc(text: impl Into<SharedString>) -> Div {
    div()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(text.into())
}

pub fn fmt_tokens(n: u64) -> String {
    if n >= 1000 {
        format!("{}k", (n as f64 / 1000.).round() as u64)
    } else {
        n.to_string()
    }
}

pub fn fmt_k(n: f64) -> String {
    format!("{:.1}k", n / 1000.)
}
