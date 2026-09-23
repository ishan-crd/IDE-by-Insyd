//! InsyDE — a native, GPU-rendered IDE for AI coding agents.

mod assets;
mod brain_view;
mod chat;
mod search;
mod terminal_view;
mod ui;
mod workspace;

use gpui::prelude::*;
use gpui::{App, Bounds, Context, Entity, Focusable, KeyBinding, TitlebarOptions, Window, WindowBounds, WindowOptions, div, point, px, size};
use insyde_core::store::Store;
use insyde_theme::{ActiveTheme, Mode};
use terminal_view::TerminalView;
use workspace::*;

/// A terminal popped out into its own window.
pub struct PopOut {
    pub view: Entity<TerminalView>,
}

impl Render for PopOut {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        div().size_full().bg(t.panel_2).child(self.view.clone())
    }
}

/// Keep gpui-component widgets (inputs, markdown, scrollbars) on our tokens.
pub fn sync_component_theme(cx: &mut App) {
    let t = cx.theme().clone();
    let mode = if t.is_dark() { gpui_component::ThemeMode::Dark } else { gpui_component::ThemeMode::Light };
    gpui_component::Theme::change(mode, None, cx);
    let c = gpui_component::Theme::global_mut(cx);
    c.font_family = insyde_theme::metrics::UI_FONT.into();
    c.font_size = px(13.);
    c.mono_font_family = insyde_theme::metrics::MONO_FONT.into();
    c.radius = px(6.);
    c.radius_lg = px(10.);
    let k = &mut c.colors;
    k.background = t.panel;
    k.foreground = t.ink;
    k.border = t.field_border;
    k.input = t.field_border;
    k.ring = t.accent;
    k.caret = t.ink;
    k.accent = t.hover;
    k.accent_foreground = t.ink;
    k.muted = t.hover_2;
    k.muted_foreground = t.ink_3;
    k.popover = t.panel;
    k.popover_foreground = t.ink;
    k.primary = t.primary;
    k.primary_foreground = t.on_primary;
    k.secondary = t.hover_2;
    k.secondary_foreground = t.ink;
    k.link = t.accent;
    k.selection = t.sel_chip;
    k.scrollbar_thumb = t.scroll_thumb;
}

fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "insyde=info,warn".into())).init();
    let store = Store::open_default().expect("open InsyDE database");
    let mode = match store.get::<String>("theme").as_deref() {
        Some("light") => Mode::Light,
        _ => Mode::Dark,
    };
    gpui_platform::application().with_assets(assets::Assets).run(move |cx: &mut App| {
        gpui_component::init(cx);
        insyde_theme::init(cx, mode);
        sync_component_theme(cx);
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-t", NewAgent, Some("Workspace")),
            KeyBinding::new("cmd-w", CloseTab, Some("Workspace")),
            KeyBinding::new("cmd-b", ToggleSidebar, Some("Workspace")),
            KeyBinding::new("cmd-j", ToggleTerminals, Some("Workspace")),
            KeyBinding::new("cmd-alt-b", ToggleRight, Some("Workspace")),
            KeyBinding::new("cmd-shift-l", ToggleTheme, Some("Workspace")),
            KeyBinding::new("ctrl-`", NewTerminal, Some("Workspace")),
            KeyBinding::new("cmd-o", OpenProject, Some("Workspace")),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1512.), px(982.)), cx);
        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some("InsyDE".into()), appears_transparent: true, traffic_light_position: Some(point(px(16.), px(17.))) }),
            window_min_size: Some(size(px(960.), px(620.))),
            ..Default::default()
        };
        cx.open_window(opts, |window, cx| {
            let ws = cx.new(|cx| Workspace::new(store.clone(), window, cx));
            ws.update(cx, |w, cx| {
                w.ensure_wt(Some(window), cx);
                w.focus_handle(cx).focus(window, cx);
            });
            cx.new(|cx| gpui_component::Root::new(ws, window, cx))
        })
        .expect("open window");
        cx.activate(true);
    });
}
