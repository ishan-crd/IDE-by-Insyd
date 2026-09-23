//! GPU terminal view. Paints the visible part of an `alacritty_terminal` grid:
//! one shaped line per row (GPUI caches shaped lines across frames), batched
//! background rects, and the cursor. It repaints only when the PTY reports new
//! output, so an idle terminal costs zero CPU.

use gpui::prelude::*;
use gpui::{
    App, Bounds, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable, Font, FontStyle,
    FontWeight, Hsla, KeyDownEvent, MouseButton, Pixels, ScrollDelta, ScrollWheelEvent,
    SharedString, TextRun, Window, canvas, div, fill, font, point, px, size,
};
use insyde_core::terminal::{
    CellFlags, CursorShape, NamedColor, Size, SpawnOptions, TermColor, TermEvent, TermMode,
    TerminalSession, default_shell,
};
use insyde_theme::{ActiveTheme, Theme, metrics};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

pub enum TerminalEvent {
    TitleChanged,
    Exited,
    Activity,
}

pub struct TerminalView {
    session: Option<Arc<TerminalSession>>,
    pub title: SharedString,
    pub sub: SharedString,
    pub exited: Option<i32>,
    pub error: Option<String>,
    focus: FocusHandle,
    font_size: f32,
    /// Command line this terminal runs (None = login shell).
    pub program: Option<(String, Vec<String>)>,
    pub cwd: PathBuf,
    pub last_activity: std::time::Instant,
    /// Text-area origin and cell size from the last paint, for mouse → cell mapping.
    geom: std::rc::Rc<std::cell::Cell<(gpui::Point<Pixels>, Pixels, Pixels)>>,
    selecting: bool,
}

impl EventEmitter<TerminalEvent> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TerminalView {
    pub fn new(
        cwd: PathBuf,
        program: Option<(String, Vec<String>)>,
        env: HashMap<String, String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let (tx, rx) = flume::unbounded::<TermEvent>();
        let notify: insyde_core::terminal::Notify = Arc::new(move |e| {
            let _ = tx.send(e);
        });
        let title: SharedString = match &program {
            Some((p, _)) => std::path::Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.clone())
                .into(),
            None => std::path::Path::new(&default_shell())
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "shell".into())
                .into(),
        };
        let sub: SharedString = cwd
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
            .into();
        let settings = insyde_core::settings::get();
        let mut env = env;
        env.insert("PATH".into(), insyde_core::agents::augmented_path());
        // Remote worktrees: the PTY runs `ssh -t host` into the worktree.
        let (local_cwd, spawn_program) = match insyde_core::remote::split(&cwd) {
            Some((host, dir)) => {
                let p = program.as_ref().map(|(p, a)| (p.as_str(), a.as_slice()));
                let (ssh, args) = insyde_core::remote::pty_command(&host, &dir, p);
                (dirs_home(), Some((ssh, args)))
            }
            None => {
                // A shell override from settings applies to plain terminals.
                let shell = settings.shell.trim().to_string();
                let program = match (&program, shell.is_empty()) {
                    (None, false) => Some((shell, vec!["-l".to_string()])),
                    _ => program.clone(),
                };
                (cwd.clone(), program)
            }
        };
        let opts = SpawnOptions {
            cwd: local_cwd,
            program: spawn_program,
            env,
            size: Size {
                cols: 80,
                rows: 24,
                cell_w: 7.,
                cell_h: 19.,
            },
            scrollback: settings.term_scrollback.clamp(100, 200_000) as usize,
        };
        let (session, error) = match TerminalSession::spawn(opts, notify) {
            Ok(s) => (Some(Arc::new(s)), None),
            Err(e) => (None, Some(e.to_string())),
        };
        cx.spawn(async move |this, cx| {
            while let Ok(ev) = rx.recv_async().await {
                let keep = this
                    .update(cx, |this, cx| {
                        match ev {
                            TermEvent::Dirty => {
                                this.last_activity = std::time::Instant::now();
                                cx.emit(TerminalEvent::Activity);
                            }
                            TermEvent::Title(t) => {
                                if !t.trim().is_empty() {
                                    this.title = t.into();
                                    cx.emit(TerminalEvent::TitleChanged);
                                }
                            }
                            TermEvent::Exited(code) => {
                                this.exited = Some(code.unwrap_or(0));
                                cx.emit(TerminalEvent::Exited);
                            }
                            TermEvent::Bell => {}
                        }
                        cx.notify();
                    })
                    .is_ok();
                if !keep {
                    break;
                }
            }
        })
        .detach();
        Self {
            session,
            title,
            sub,
            exited: None,
            error,
            focus: cx.focus_handle(),
            font_size: settings.term_font_size.clamp(8., 28.),
            program,
            cwd,
            last_activity: std::time::Instant::now() - std::time::Duration::from_secs(3600),
            geom: Default::default(),
            selecting: false,
        }
    }

    pub fn session(&self) -> Option<&Arc<TerminalSession>> {
        self.session.as_ref()
    }

    pub fn send_text(&self, s: &str) {
        if let Some(sess) = &self.session {
            sess.write(s.as_bytes().to_vec());
        }
    }

    pub fn tail(&self, n: usize) -> String {
        self.session
            .as_ref()
            .map(|s| s.tail_text(n))
            .unwrap_or_default()
    }

    pub fn is_busy(&self) -> bool {
        self.exited.is_none() && self.last_activity.elapsed().as_secs() < 3
    }

    fn on_key(&mut self, ev: &KeyDownEvent, _w: &mut Window, cx: &mut Context<Self>) {
        let Some(sess) = self.session.clone() else {
            return;
        };
        let k = &ev.keystroke;
        let m = k.modifiers;
        if m.platform {
            match k.key.as_str() {
                "v" => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                        sess.paste(&text);
                    }
                }
                "c" => {
                    if let Some(text) = sess.selected_text().or_else(|| sess.take_clipboard()) {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                }
                "k" => sess.write(b"\x0c".to_vec()),
                "=" | "+" => self.font_size = (self.font_size + 1.).min(24.),
                "-" => self.font_size = (self.font_size - 1.).max(8.),
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let app_cursor = sess.term.lock().mode().contains(TermMode::APP_CURSOR);
        // Option as Meta: ⌥B sends ESC b (readline word jumps). Otherwise ⌥
        // types the composed character (e.g. ∫), as in macOS apps.
        let meta = insyde_core::settings::get().option_as_meta;
        let (ch, alt) = match (m.alt, meta) {
            (true, true) if k.key.chars().count() == 1 => (Some(k.key.as_str()), true),
            (true, false) => (k.key_char.as_deref(), false),
            _ => (k.key_char.as_deref(), m.alt),
        };
        if let Some(bytes) = key_bytes(&k.key, ch, m.control, alt, m.shift, app_cursor) {
            sess.clear_selection();
            sess.scroll_to_bottom();
            sess.write(bytes);
            cx.stop_propagation();
        }
    }

    fn cell_at(&self, p: gpui::Point<Pixels>) -> (usize, usize) {
        let (origin, cw, lh) = self.geom.get();
        let x = f32::from(p.x - origin.x).max(0.) / f32::from(cw).max(1.);
        let y = f32::from(p.y - origin.y).max(0.) / f32::from(lh).max(1.);
        (x as usize, y as usize)
    }

    fn on_scroll(&mut self, ev: &ScrollWheelEvent, _w: &mut Window, cx: &mut Context<Self>) {
        let Some(sess) = &self.session else { return };
        let line_h = self.font_size * insyde_core::settings::get().term_line_height.clamp(1., 2.5);
        let dy = match ev.delta {
            ScrollDelta::Pixels(p) => f32::from(p.y) / line_h,
            ScrollDelta::Lines(l) => l.y * 3.,
        };
        let lines = dy.round() as i32;
        if lines == 0 {
            return;
        }
        let mode = *sess.term.lock().mode();
        if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            let seq: &[u8] = if lines > 0 { b"\x1bOA" } else { b"\x1bOB" };
            let mut v = Vec::new();
            for _ in 0..lines.unsigned_abs().min(20) {
                v.extend_from_slice(seq);
            }
            sess.write(v);
        } else {
            sess.scroll(lines);
        }
        cx.notify();
    }
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Keystroke → bytes for the PTY (xterm conventions).
fn key_bytes(
    key: &str,
    ch: Option<&str>,
    ctrl: bool,
    alt: bool,
    shift: bool,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    let arrow = |c: char| -> Vec<u8> {
        if shift || alt || ctrl {
            let m = 1 + shift as u8 + 2 * alt as u8 + 4 * ctrl as u8;
            format!("\x1b[1;{m}{c}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{c}").into_bytes()
        } else {
            format!("\x1b[{c}").into_bytes()
        }
    };
    let v: Vec<u8> = match key {
        "enter" => {
            if alt {
                b"\x1b\r".to_vec()
            } else if shift {
                b"\x1b[13;2u".to_vec()
            } else {
                b"\r".to_vec()
            }
        }
        "backspace" => {
            if alt || ctrl {
                b"\x17".to_vec()
            } else {
                b"\x7f".to_vec()
            }
        }
        "tab" => {
            if shift {
                b"\x1b[Z".to_vec()
            } else {
                b"\t".to_vec()
            }
        }
        "escape" => b"\x1b".to_vec(),
        "up" => arrow('A'),
        "down" => arrow('B'),
        "right" => arrow('C'),
        "left" => arrow('D'),
        "home" => arrow('H'),
        "end" => arrow('F'),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "insert" => b"\x1b[2~".to_vec(),
        "f1" => b"\x1bOP".to_vec(),
        "f2" => b"\x1bOQ".to_vec(),
        "f3" => b"\x1bOR".to_vec(),
        "f4" => b"\x1bOS".to_vec(),
        "space" if ctrl => vec![0],
        _ => {
            if ctrl && key.len() == 1 {
                let c = key.as_bytes()[0].to_ascii_lowercase();
                let b = match c {
                    b'a'..=b'z' => c - b'a' + 1,
                    b'[' => 27,
                    b'\\' => 28,
                    b']' => 29,
                    b'^' => 30,
                    b'_' => 31,
                    _ => return None,
                };
                if alt { vec![0x1b, b] } else { vec![b] }
            } else if let Some(s) = ch {
                let mut v = Vec::with_capacity(s.len() + 1);
                if alt {
                    v.push(0x1b);
                }
                v.extend_from_slice(s.as_bytes());
                v
            } else if key == "space" {
                b" ".to_vec()
            } else {
                return None;
            }
        }
    };
    Some(v)
}

/// ANSI palette tuned to the design's neutral inks (light and dark).
fn ansi(t: &Theme, idx: usize) -> Hsla {
    let hex = |v: u32| -> Hsla { gpui::rgb(v).into() };
    let dark = t.is_dark();
    match idx {
        0 => {
            if dark {
                hex(0x3A3A38)
            } else {
                hex(0x1A1A19)
            }
        }
        1 => t.err,
        2 => t.ok,
        3 => t.warn,
        4 => t.palette.blue,
        5 => t.palette.purple,
        6 => t.palette.teal,
        7 => t.ink_2,
        8 => t.ink_3,
        9 => {
            if dark {
                hex(0xFF8A8F)
            } else {
                hex(0xD6453C)
            }
        }
        10 => {
            if dark {
                hex(0x6BE3AE)
            } else {
                hex(0x2E9E6B)
            }
        }
        11 => {
            if dark {
                hex(0xF7D06E)
            } else {
                hex(0xB7861F)
            }
        }
        12 => {
            if dark {
                hex(0x7FA8F0)
            } else {
                hex(0x2F6BEB)
            }
        }
        13 => {
            if dark {
                hex(0xA88CF0)
            } else {
                hex(0x7C5CD6)
            }
        }
        14 => {
            if dark {
                hex(0x5CC6C8)
            } else {
                hex(0x2A9D9F)
            }
        }
        _ => t.ink,
    }
}

fn indexed(t: &Theme, i: u8) -> Hsla {
    match i {
        0..=15 => ansi(t, i as usize),
        16..=231 => {
            let i = i - 16;
            let c = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            gpui::Rgba {
                r: c(i / 36) as f32 / 255.,
                g: c((i / 6) % 6) as f32 / 255.,
                b: c(i % 6) as f32 / 255.,
                a: 1.,
            }
            .into()
        }
        _ => {
            let v = (8 + (i - 232) as u32 * 10) as f32 / 255.;
            gpui::Rgba {
                r: v,
                g: v,
                b: v,
                a: 1.,
            }
            .into()
        }
    }
}

fn color(t: &Theme, c: TermColor, fg: bool) -> Option<Hsla> {
    Some(match c {
        TermColor::Spec(rgb) => gpui::Rgba {
            r: rgb.r as f32 / 255.,
            g: rgb.g as f32 / 255.,
            b: rgb.b as f32 / 255.,
            a: 1.,
        }
        .into(),
        TermColor::Indexed(i) => indexed(t, i),
        TermColor::Named(n) => match n {
            NamedColor::Foreground | NamedColor::BrightForeground => t.ink_2,
            NamedColor::DimForeground => t.ink_3,
            NamedColor::Background => return if fg { Some(t.panel_2) } else { None },
            NamedColor::Cursor => t.ink,
            other => {
                let i = other as usize;
                if i < 16 { ansi(t, i) } else { t.ink_2 }
            }
        },
    })
}

struct Row {
    y: usize,
    text: String,
    runs: Vec<TextRun>,
    bgs: Vec<(usize, usize, Hsla)>,
}

struct Frame {
    rows: Vec<Row>,
    cursor: Option<(usize, usize, CursorShape)>,
    cell: gpui::Size<Pixels>,
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let focused = self.focus.is_focused(window);
        let base = div()
            .id("terminal")
            .key_context("Terminal")
            .track_focus(&self.focus)
            // Fill the (relative) host exactly: a percentage height inside a flex
            // item doesn't resolve, which left the hitbox shorter than the text.
            .absolute()
            .inset_0()
            .overflow_hidden()
            .px(px(12.))
            .py(px(8.))
            .cursor_text()
            .on_key_down(cx.listener(Self::on_key))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &gpui::MouseDownEvent, w, cx| {
                    this.focus.focus(w, cx);
                    if let Some(s) = &this.session {
                        let (c, r) = this.cell_at(e.position);
                        s.select_start(c, r, e.click_count);
                        this.selecting = true;
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, cx| {
                if this.selecting
                    && e.dragging()
                    && let Some(s) = &this.session
                {
                    let (c, r) = this.cell_at(e.position);
                    s.select_update(c, r);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.selecting = false;
                    // A plain click leaves an empty selection; drop it.
                    if let Some(s) = &this.session {
                        match s.selected_text() {
                            None => s.clear_selection(),
                            Some(text) if insyde_core::settings::get().copy_on_select => {
                                cx.write_to_clipboard(ClipboardItem::new_string(text));
                            }
                            Some(_) => {}
                        }
                    }
                    cx.notify();
                }),
            );
        let Some(sess) = self.session.clone() else {
            return base.child(
                div()
                    .text_color(t.err)
                    .text_size(metrics::TEXT_SM)
                    .child(self.error.clone().unwrap_or_default()),
            );
        };
        let font_size = px(self.font_size);
        let settings = insyde_core::settings::get();
        let line_h = px((self.font_size * settings.term_line_height.clamp(1., 2.5)).round());
        let family = if settings.term_font.trim().is_empty() {
            metrics::MONO_FONT.to_string()
        } else {
            settings.term_font.trim().to_string()
        };
        let mono = font(family);
        let geom = self.geom.clone();
        base.child(
            canvas(
                move |bounds, window, cx| {
                    // Measure a cell once per frame (cheap: cached glyph advance).
                    let ts = window.text_system();
                    let fid = ts.resolve_font(&mono);
                    let cell_w = ts
                        .advance(fid, font_size, 'm')
                        .map(|s| s.width)
                        .unwrap_or(px(7.));
                    geom.set((bounds.origin, cell_w, line_h));
                    let cols = ((bounds.size.width / cell_w).floor() as u16).max(2);
                    let rows = ((bounds.size.height / line_h).floor() as u16).max(1);
                    sess.resize(Size {
                        cols,
                        rows,
                        cell_w: f32::from(cell_w),
                        cell_h: f32::from(line_h),
                    });
                    sess.ack();
                    let t = cx.theme().clone();
                    let term = sess.term.lock();
                    let content = term.renderable_content();
                    let offset = content.display_offset as i32;
                    let selection = content.selection;
                    let sel_bg = t.sel_chip;
                    let mut rows_out: Vec<Row> = Vec::with_capacity(rows as usize);
                    let mut cur_line = i32::MIN;
                    for ind in content.display_iter {
                        let y = ind.point.line.0 + offset;
                        if y < 0 || y >= rows as i32 {
                            continue;
                        }
                        if y != cur_line {
                            cur_line = y;
                            rows_out.push(Row {
                                y: y as usize,
                                text: String::new(),
                                runs: Vec::new(),
                                bgs: Vec::new(),
                            });
                        }
                        let row = rows_out.last_mut().unwrap();
                        let cell = ind.cell;
                        if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                            continue;
                        }
                        let x = ind.point.column.0;
                        let (mut fg, mut bg) = (cell.fg, cell.bg);
                        if cell.flags.contains(CellFlags::INVERSE) {
                            std::mem::swap(&mut fg, &mut bg);
                        }
                        let mut fgc = color(&t, fg, true).unwrap_or(t.ink_2);
                        if cell.flags.contains(CellFlags::DIM) {
                            fgc.a *= 0.6;
                        }
                        let selected = selection.is_some_and(|s| s.contains(ind.point));
                        let bg_color = if selected {
                            Some(sel_bg)
                        } else {
                            color(&t, bg, false)
                        };
                        if let Some(bgc) = bg_color {
                            match row.bgs.last_mut() {
                                Some((_, end, c)) if *end == x && *c == bgc => *end = x + 1,
                                _ => row.bgs.push((x, x + 1, bgc)),
                            }
                        }
                        // Pad skipped columns so text stays grid-aligned.
                        let have = row.text.chars().count();
                        if x > have {
                            let pad = x - have;
                            row.text.extend(std::iter::repeat_n(' ', pad));
                            push_run(&mut row.runs, pad, &mono, fgc, false, false);
                        }
                        let c = if cell.flags.contains(CellFlags::HIDDEN) {
                            ' '
                        } else {
                            cell.c
                        };
                        let len = c.len_utf8();
                        row.text.push(c);
                        push_run(
                            &mut row.runs,
                            len,
                            &mono,
                            fgc,
                            cell.flags.contains(CellFlags::BOLD),
                            cell.flags.contains(CellFlags::ITALIC),
                        );
                    }
                    let cursor = if offset == 0 && content.cursor.shape != CursorShape::Hidden {
                        let p = content.cursor.point;
                        (p.line.0 >= 0).then_some((
                            p.column.0,
                            p.line.0 as usize,
                            content.cursor.shape,
                        ))
                    } else {
                        None
                    };
                    Frame {
                        rows: rows_out,
                        cursor,
                        cell: size(cell_w, line_h),
                    }
                },
                move |bounds, frame, window, cx| {
                    let t = cx.theme().clone();
                    let origin = bounds.origin;
                    let cw = frame.cell.width;
                    for row in &frame.rows {
                        let y = origin.y + line_h * row.y as f32;
                        for (x0, x1, c) in &row.bgs {
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(origin.x + cw * *x0 as f32, y),
                                    size(cw * (*x1 - *x0) as f32, line_h),
                                ),
                                *c,
                            ));
                        }
                    }
                    if let Some((cx_, cy, shape)) = frame.cursor {
                        let p = point(origin.x + cw * cx_ as f32, origin.y + line_h * cy as f32);
                        let r = match (shape, focused) {
                            (_, false) => None,
                            (CursorShape::Beam, _) => Some(Bounds::new(p, size(px(1.5), line_h))),
                            (CursorShape::Underline, _) => Some(Bounds::new(
                                point(p.x, p.y + line_h - px(2.)),
                                size(cw, px(2.)),
                            )),
                            _ => Some(Bounds::new(p, size(cw, line_h))),
                        };
                        match r {
                            Some(r) => window.paint_quad(fill(r, t.ink.opacity(0.85))),
                            None => window.paint_quad(gpui::outline(
                                Bounds::new(p, size(cw, line_h)),
                                t.ink_3,
                                gpui::BorderStyle::Solid,
                            )),
                        }
                    }
                    for row in frame.rows {
                        if row.text.trim_end().is_empty() {
                            continue;
                        }
                        let y = origin.y + line_h * row.y as f32;
                        let shaped = window.text_system().shape_line(
                            row.text.into(),
                            font_size,
                            &row.runs,
                            Some(cw),
                        );
                        let _ = shaped.paint(
                            point(origin.x, y),
                            line_h,
                            gpui::TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    }
                    // Paint the block cursor's glyph in the background color for contrast.
                },
            )
            .size_full(),
        )
    }
}

fn push_run(
    runs: &mut Vec<TextRun>,
    len: usize,
    base: &Font,
    color: Hsla,
    bold: bool,
    italic: bool,
) {
    if let Some(last) = runs.last_mut() {
        let same_style = (last.font.weight == FontWeight::BOLD) == bold
            && (last.font.style == FontStyle::Italic) == italic;
        if last.color == color && same_style {
            last.len += len;
            return;
        }
    }
    let mut f = base.clone();
    if bold {
        f.weight = FontWeight::BOLD;
    }
    if italic {
        f.style = FontStyle::Italic;
    }
    runs.push(TextRun {
        len,
        font: f,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    });
}
