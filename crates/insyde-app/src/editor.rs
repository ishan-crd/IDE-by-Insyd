//! File editor: gpui-component's rope-backed code editor (tree-sitter
//! highlighting, line numbers, search) with load/save, a dirty marker, and
//! reload-on-agent-edit that never clobbers unsaved work.

use crate::ui::{self, icon};
use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, FontWeight, SharedString, Subscription, Window, div, px,
};
use gpui_component::input::{Editor, EditorState, InputEvent};
use insyde_theme::{ActiveTheme, metrics};
use std::path::PathBuf;

/// Files larger than this open read-only (the editor keeps the whole rope in memory).
const MAX_EDIT_BYTES: usize = 2 << 20;

pub enum EditorEvent {
    Saved(String),
    Error(String),
}

pub struct FileEditor {
    pub root: PathBuf,
    pub rel: String,
    state: Entity<EditorState>,
    /// Text as last loaded from / written to disk.
    disk: String,
    pub dirty: bool,
    readonly: bool,
    loading: bool,
    _subs: Vec<Subscription>,
}

impl EventEmitter<EditorEvent> for FileEditor {}

impl FileEditor {
    pub fn open(
        root: PathBuf,
        rel: String,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = insyde_core::settings::get();
        let ext = rel.rsplit('.').next().unwrap_or("").to_string();
        let state = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(ext)
                .line_number(settings.editor_line_numbers)
                .soft_wrap(settings.editor_soft_wrap)
                .tab_size(gpui_component::input::TabSize {
                    tab_size: settings.editor_tab_size.clamp(1, 16) as usize,
                    hard_tabs: false,
                })
                // No fold column: it widened the gutter in a narrow side panel.
                .folding(false)
                .searchable(true)
        });
        let subs = vec![
            cx.subscribe(&state, |this: &mut Self, s, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) && !this.loading {
                    let dirty = s.read(cx).value().as_ref() != this.disk.as_str();
                    if dirty != this.dirty {
                        this.dirty = dirty;
                        cx.notify();
                    }
                }
            }),
        ];
        let mut this = Self {
            root,
            rel,
            state,
            disk: String::new(),
            dirty: false,
            readonly: false,
            loading: true,
            _subs: subs,
        };
        this.load(line, window, cx);
        this
    }

    fn path(&self) -> PathBuf {
        self.root.join(&self.rel)
    }

    fn load(&mut self, line: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.loading = true;
        let path = self.path();
        let task = cx.background_spawn(async move {
            let bytes = insyde_core::remote::read_file(&path).map_err(|e| e.to_string())?;
            if bytes.contains(&0) {
                return Err("Binary file".to_string());
            }
            Ok((
                String::from_utf8_lossy(&bytes).into_owned(),
                bytes.len() > MAX_EDIT_BYTES,
            ))
        });
        cx.spawn_in(window, async move |this, cx| {
            let res = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match res {
                    Ok((text, big)) => {
                        this.readonly = big;
                        this.disk = text.clone();
                        this.state.update(cx, |s, cx| {
                            s.set_value(text, window, cx);
                            if let Some(l) = line {
                                s.set_cursor_position(
                                    gpui_component::input::Position::new(
                                        l.saturating_sub(1) as u32,
                                        0,
                                    ),
                                    window,
                                    cx,
                                );
                            }
                        });
                        this.dirty = false;
                    }
                    Err(e) => cx.emit(EditorEvent::Error(format!("{}: {e}", this.rel))),
                }
                this.loading = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Reload from disk (after an agent edited the file) unless there are unsaved edits.
    pub fn reload_if_clean(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dirty || self.loading {
            return;
        }
        let path = self.path();
        if insyde_core::remote::read_to_string(&path).is_ok_and(|t| t != self.disk) {
            self.load(None, window, cx);
        }
    }

    pub fn save(&mut self, cx: &mut Context<Self>) {
        if self.readonly || !self.dirty {
            return;
        }
        let text = self.state.read(cx).value().to_string();
        let path = self.path();
        let rel = self.rel.clone();
        match insyde_core::remote::write_file(&path, text.as_bytes()) {
            Ok(()) => {
                self.disk = text;
                self.dirty = false;
                cx.emit(EditorEvent::Saved(rel));
            }
            Err(e) => cx.emit(EditorEvent::Error(format!("Couldn't save {rel}: {e}"))),
        }
        cx.notify();
    }

    fn revert(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.disk.clone();
        self.loading = true;
        self.state.update(cx, |s, cx| s.set_value(text, window, cx));
        self.loading = false;
        self.dirty = false;
        cx.notify();
    }
}

impl Render for FileEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let header = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(14.))
            .pb(px(8.))
            .child(icon("file", 12., t.ink_3))
            .child(
                ui::trunc(SharedString::from(self.rel.clone()))
                    .flex_1()
                    .text_size(metrics::TEXT_SM)
                    .text_color(t.ink),
            )
            .when(self.dirty, |d| d.child(ui::dot(t.warn, 7.)))
            .when(self.readonly, |d| {
                d.child(
                    div()
                        .text_size(metrics::TEXT_XS)
                        .text_color(t.ink_3)
                        .child("read-only · large file"),
                )
            })
            .when(self.dirty, |d| {
                d.child(
                    ui::small_button("revert", "Revert", &t)
                        .on_click(cx.listener(|this, _, w, cx| this.revert(w, cx))),
                )
                .child(
                    ui::primary_button("save", &t)
                        .h(px(26.))
                        .px(px(10.))
                        .text_size(metrics::TEXT_SM)
                        .font_weight(FontWeight::MEDIUM)
                        .child("Save")
                        .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                )
            });
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(t.line)
                    .bg(t.panel_2)
                    .child(
                        Editor::new(&self.state)
                            .appearance(false)
                            .text_size(metrics::TEXT_MONO)
                            .readonly(self.readonly)
                            .h(gpui::relative(1.)),
                    ),
            )
    }
}
