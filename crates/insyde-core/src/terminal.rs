//! PTY-backed terminal sessions on top of `alacritty_terminal` (the same
//! engine Zed uses). The PTY reader runs on alacritty's own thread and parses
//! straight into the grid; the UI only takes a short read lock per frame to
//! paint the visible rows. Change notifications are coalesced: at most one
//! pending "dirty" signal is queued no matter how fast output arrives.

use alacritty_terminal::event::{Event as AlacEvent, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty;
use anyhow::Result;
use parking_lot::Mutex;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub use alacritty_terminal::event::Event as RawEvent;
pub use alacritty_terminal::term::TermMode;
pub use alacritty_terminal::term::cell::{Cell, Flags as CellFlags};
pub use alacritty_terminal::vte::ansi::{Color as TermColor, CursorShape, NamedColor, Rgb};
pub use alacritty_terminal::{Term as Grid, index};

/// What the UI needs to hear about. Sent through the callback given to
/// [`TerminalSession::spawn`].
#[derive(Clone, Debug)]
pub enum TermEvent {
    /// New output; repaint when convenient.
    Dirty,
    Title(String),
    Bell,
    Exited(Option<i32>),
}

pub type Notify = Arc<dyn Fn(TermEvent) + Send + Sync>;

#[derive(Clone)]
pub struct Listener {
    notify: Notify,
    pending: Arc<AtomicBool>,
    writer: Arc<Mutex<Option<EventLoopSender>>>,
    clipboard: Arc<Mutex<Option<String>>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: AlacEvent) {
        match event {
            AlacEvent::Wakeup => {
                if !self.pending.swap(true, Ordering::AcqRel) {
                    (self.notify)(TermEvent::Dirty);
                }
            }
            AlacEvent::Title(t) => (self.notify)(TermEvent::Title(t)),
            AlacEvent::Bell => (self.notify)(TermEvent::Bell),
            AlacEvent::ChildExit(s) => (self.notify)(TermEvent::Exited(s.code())),
            AlacEvent::Exit => (self.notify)(TermEvent::Exited(None)),
            AlacEvent::PtyWrite(s) => {
                if let Some(w) = self.writer.lock().as_ref() {
                    let _ = w.send(Msg::Input(Cow::Owned(s.into_bytes())));
                }
            }
            AlacEvent::ClipboardStore(_, s) => *self.clipboard.lock() = Some(s),
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
    pub cell_w: f32,
    pub cell_h: f32,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }
    fn screen_lines(&self) -> usize {
        self.rows as usize
    }
    fn columns(&self) -> usize {
        self.cols as usize
    }
}

impl From<Size> for WindowSize {
    fn from(s: Size) -> Self {
        WindowSize { num_lines: s.rows, num_cols: s.cols, cell_width: s.cell_w as u16, cell_height: s.cell_h as u16 }
    }
}

pub struct SpawnOptions {
    pub cwd: PathBuf,
    /// `None` = the user's login shell.
    pub program: Option<(String, Vec<String>)>,
    pub env: HashMap<String, String>,
    pub size: Size,
    pub scrollback: usize,
}

pub struct TerminalSession {
    pub term: Arc<FairMutex<Term<Listener>>>,
    sender: EventLoopSender,
    pending: Arc<AtomicBool>,
    size: Size,
    pub cwd: PathBuf,
    pub pid: u32,
    clipboard: Arc<Mutex<Option<String>>>,
}

impl TerminalSession {
    pub fn spawn(opts: SpawnOptions, notify: Notify) -> Result<Self> {
        let mut env = opts.env;
        env.insert("TERM".into(), "xterm-256color".into());
        env.insert("COLORTERM".into(), "truecolor".into());
        env.insert("TERM_PROGRAM".into(), "InsyDE".into());
        let pty_opts = tty::Options {
            shell: opts.program.map(|(p, a)| tty::Shell::new(p, a)),
            working_directory: Some(opts.cwd.clone()),
            drain_on_exit: true,
            env,
            #[cfg(windows)]
            escape_args: true,
        };
        let pending = Arc::new(AtomicBool::new(false));
        let writer = Arc::new(Mutex::new(None));
        let clipboard = Arc::new(Mutex::new(None));
        let listener = Listener { notify, pending: pending.clone(), writer: writer.clone(), clipboard: clipboard.clone() };
        let config = Config { scrolling_history: opts.scrollback, ..Config::default() };
        let term = Arc::new(FairMutex::new(Term::new(config, &opts.size, listener.clone())));
        let pty = tty::new(&pty_opts, opts.size.into(), 0)?;
        #[cfg(unix)]
        let pid = pty.child().id();
        #[cfg(not(unix))]
        let pid = 0;
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)?;
        let sender = event_loop.channel();
        *writer.lock() = Some(sender.clone());
        event_loop.spawn();
        Ok(Self { term, sender, pending, size: opts.size, cwd: opts.cwd, pid, clipboard })
    }

    /// Call when a frame consumed the dirty signal, re-arming notifications.
    pub fn ack(&self) {
        self.pending.store(false, Ordering::Release);
    }

    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let _ = self.sender.send(Msg::Input(bytes.into()));
    }

    pub fn paste(&self, text: &str) {
        let bracketed = self.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
        let clean = text.replace("\r\n", "\r").replace('\n', "\r");
        if bracketed {
            self.write(format!("\x1b[200~{}\x1b[201~", clean.replace('\x1b', "")).into_bytes());
        } else {
            self.write(clean.into_bytes());
        }
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn resize(&mut self, size: Size) {
        if size == self.size || size.cols < 2 || size.rows < 1 {
            return;
        }
        self.size = size;
        let _ = self.sender.send(Msg::Resize(size.into()));
        self.term.lock().resize(size);
    }

    pub fn scroll(&self, lines: i32) {
        self.term.lock().scroll_display(Scroll::Delta(lines));
    }

    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    pub fn take_clipboard(&self) -> Option<String> {
        self.clipboard.lock().take()
    }

    /// Plain text of the last `n` non-empty lines (scrollback included), used
    /// for agent hand-off and the "Problems" count.
    pub fn tail_text(&self, n: usize) -> String {
        let term = self.term.lock();
        let grid = term.grid();
        let cols = grid.columns();
        let top = -(grid.history_size() as i32);
        let bottom = grid.screen_lines() as i32 - 1;
        let mut lines: Vec<String> = Vec::with_capacity(n);
        let mut y = bottom;
        while y >= top && lines.len() < n {
            let row = &grid[Line(y)];
            let mut s = String::with_capacity(cols);
            for x in 0..cols {
                let c = &row[Column(x)];
                if !c.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                    s.push(c.c);
                }
            }
            let t = s.trim_end().to_string();
            if !t.is_empty() || !lines.is_empty() {
                lines.push(t);
            }
            y -= 1;
        }
        lines.reverse();
        lines.join("\n")
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
    }
}

/// Keep the `Notifier` type reachable for callers implementing custom input.
pub type PtyNotifier = Notifier;

/// The user's login shell.
pub fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| if cfg!(windows) { "powershell.exe".into() } else { "/bin/zsh".into() })
}
