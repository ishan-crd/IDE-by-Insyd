//! Browser terminals: a raw PTY whose bytes go straight to xterm.js.
//!
//! Unlike the desktop terminal (which parses into an alacritty grid), the
//! browser does its own emulation, so the server only pumps bytes. Output is
//! kept in a bounded replay buffer so a tab that reconnects (or a second
//! device) sees the current screen instead of a blank terminal.

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{self, Pty};
use anyhow::Result;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Bytes of scrollback replayed to a newly attached client.
const REPLAY: usize = 512 * 1024;

pub struct WebTerm {
    pub id: u64,
    pub worktree: PathBuf,
    pub title: String,
    pty: Mutex<Pty>,
    writer: Mutex<File>,
    replay: Mutex<VecDeque<u8>>,
    pub alive: AtomicBool,
}

/// What to run: the login shell, or a command followed by a shell.
pub enum Program {
    Shell,
    Command(String),
}

impl WebTerm {
    /// Spawn the PTY. `on_output` gets every chunk; `on_exit` fires once.
    pub fn spawn(
        id: u64,
        worktree: &Path,
        program: Program,
        cols: u16,
        rows: u16,
        on_output: impl Fn(&[u8]) + Send + 'static,
        on_exit: impl FnOnce() + Send + 'static,
    ) -> Result<Arc<Self>> {
        let settings = crate::settings::get();
        let shell = if settings.shell.trim().is_empty() {
            crate::terminal::default_shell()
        } else {
            settings.shell.trim().to_string()
        };
        let mut env: HashMap<String, String> = settings.env_pairs().into_iter().collect();
        env.insert("TERM".into(), "xterm-256color".into());
        env.insert("COLORTERM".into(), "truecolor".into());
        env.insert("TERM_PROGRAM".into(), "InsyDE".into());
        env.insert("PATH".into(), crate::agents::augmented_path());
        env.insert(
            "INSYDE_WORKTREE".into(),
            worktree.to_string_lossy().into_owned(),
        );
        let title = match &program {
            Program::Shell => Path::new(&shell)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "shell".into()),
            Program::Command(c) => c.split_whitespace().take(2).collect::<Vec<_>>().join(" "),
        };
        let (cwd, shell_cmd) = match crate::remote::split(worktree) {
            Some((host, dir)) => {
                // Remote worktrees: `ssh -t host` into the worktree; a command
                // runs through the remote login shell, then leaves a shell open.
                let (ssh, mut a) = crate::remote::pty_command(&host, &dir, None);
                if let (Program::Command(c), Some(last)) = (&program, a.last_mut()) {
                    *last = format!(
                        "cd {} && {c}; exec \"${{SHELL:-/bin/sh}}\" -l",
                        crate::remote::quote_path(&dir)
                    );
                }
                (
                    dirs::home_dir().unwrap_or_else(|| "/".into()),
                    tty::Shell::new(ssh, a),
                )
            }
            None => {
                let args = match &program {
                    Program::Shell => vec!["-l".to_string()],
                    Program::Command(c) => {
                        vec![
                            "-lc".to_string(),
                            format!("{c}; exec {} -l", crate::remote::quote(&shell)),
                        ]
                    }
                };
                (worktree.to_path_buf(), tty::Shell::new(shell.clone(), args))
            }
        };
        let opts = tty::Options {
            shell: Some(shell_cmd),
            working_directory: Some(cwd),
            drain_on_exit: true,
            env,
            #[cfg(windows)]
            escape_args: true,
        };
        let pty = tty::new(&opts, window(cols, rows), 0)?;
        let reader = pty.file().try_clone()?;
        let writer = pty.file().try_clone()?;
        set_blocking(&reader);
        let term = Arc::new(Self {
            id,
            worktree: worktree.to_path_buf(),
            title,
            pty: Mutex::new(pty),
            writer: Mutex::new(writer),
            replay: Mutex::new(VecDeque::with_capacity(64 * 1024)),
            alive: AtomicBool::new(true),
        });
        let weak = Arc::downgrade(&term);
        std::thread::Builder::new()
            .name("web-pty".into())
            .spawn(move || {
                let mut reader = reader;
                let mut buf = vec![0u8; 16 * 1024];
                loop {
                    let n = match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let Some(t) = weak.upgrade() else { break };
                    // Sent while holding the replay lock, so an attach (which
                    // snapshots under the same lock) never misses or doubles bytes.
                    let mut r = t.replay.lock();
                    r.extend(&buf[..n]);
                    let over = r.len().saturating_sub(REPLAY);
                    r.drain(..over);
                    on_output(&buf[..n]);
                    drop(r);
                }
                if let Some(t) = weak.upgrade() {
                    t.alive.store(false, Ordering::Relaxed);
                }
                on_exit();
            })?;
        Ok(term)
    }

    pub fn write(&self, data: &[u8]) {
        let _ = self.writer.lock().write_all(data);
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        self.pty.lock().on_resize(window(cols, rows));
    }

    /// Run `f` with the replay buffer (oldest first) while output is paused,
    /// so a new subscriber can be added and sent the backlog atomically.
    pub fn with_replay<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        let r = self.replay.lock();
        let (a, b) = r.as_slices();
        f(&[a, b].concat())
    }
}

fn window(cols: u16, rows: u16) -> WindowSize {
    WindowSize {
        num_cols: cols.clamp(2, 500),
        num_lines: rows.clamp(1, 300),
        cell_width: 8,
        cell_height: 16,
    }
}

/// alacritty opens the PTY non-blocking for its poll loop; our reader
/// thread wants plain blocking reads.
#[cfg(unix)]
fn set_blocking(f: &File) {
    use std::os::fd::AsRawFd;
    let fd = f.as_raw_fd();
    // SAFETY: fcntl on a valid, owned descriptor with standard flags.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
        }
    }
}

#[cfg(not(unix))]
fn set_blocking(_: &File) {}
