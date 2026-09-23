//! Remote (SSH) locations.
//!
//! A remote path is written `ssh://<host>/<absolute path>` and flows through
//! the rest of the code like any `PathBuf` (joins keep working). Git, file
//! access, terminals and agents check [`split`] and run the same operation on
//! the host through one multiplexed SSH connection (ControlMaster), so each
//! call costs a round-trip, not a new handshake.
//!
//! The `ssh` binary can be replaced with `INSYDE_SSH` (used by tests).

use anyhow::{Context, Result, bail};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const SCHEME: &str = "ssh://";

/// `(host, remote absolute path)` for an `ssh://` path.
pub fn split(path: &Path) -> Option<(String, String)> {
    let s = path.to_str()?;
    let rest = s.strip_prefix(SCHEME).or_else(|| s.strip_prefix("ssh:/"))?;
    let (host, p) = rest.split_once('/').unwrap_or((rest, ""));
    if host.is_empty() {
        return None;
    }
    let p = p.trim_start_matches('/');
    let remote = if p.starts_with('~') {
        p.to_string()
    } else {
        format!("/{p}")
    };
    Some((host.to_string(), remote))
}

pub fn is_remote(path: &Path) -> bool {
    split(path).is_some()
}

/// Build `ssh://host/path` from a host and a remote absolute path.
pub fn join(host: &str, remote: &str) -> PathBuf {
    PathBuf::from(format!("{SCHEME}{host}/{}", remote.trim_start_matches('/')))
}

/// Parse user input: `ssh://host/path`, `host:/path` or `user@host:/path`.
pub fn parse_target(s: &str) -> Option<PathBuf> {
    let s = s.trim();
    if s.starts_with(SCHEME) {
        return split(Path::new(s)).map(|(h, p)| join(&h, &p));
    }
    let (host, path) = s.split_once(':')?;
    if host.is_empty() || path.is_empty() || host.contains('/') {
        return None;
    }
    let path = if path.starts_with('/') || path.starts_with('~') {
        path.to_string()
    } else {
        format!("~/{path}")
    };
    Some(join(host, &path))
}

/// The remote form of `path` if it lives on the same host as `like`, for
/// passing paths as arguments to remote commands.
pub fn arg(path: &Path) -> String {
    split(path)
        .map(|(_, p)| p)
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// POSIX single-quote a string for a remote shell.
pub fn quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Quote a remote path, keeping a leading `~/` expandable.
pub fn quote_path(p: &str) -> String {
    match p.strip_prefix("~/") {
        Some(rest) => format!("~/{}", quote(rest)),
        None if p == "~" => "~".into(),
        None => quote(p),
    }
}

fn control_path() -> String {
    let dir = crate::store::data_dir().join("ssh");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("%C").to_string_lossy().into_owned()
}

/// SSH options shared by every connection (multiplexed, non-interactive).
pub fn ssh_options(interactive: bool) -> Vec<String> {
    let mut v = vec![
        "-o".into(),
        "ControlMaster=auto".into(),
        "-o".into(),
        format!("ControlPath={}", control_path()),
        "-o".into(),
        "ControlPersist=600".into(),
        "-o".into(),
        "ServerAliveInterval=30".into(),
    ];
    if interactive {
        v.push("-t".into());
    } else {
        v.extend(["-o".into(), "BatchMode=yes".into(), "-T".into()]);
    }
    v
}

pub fn ssh_program() -> String {
    std::env::var("INSYDE_SSH").unwrap_or_else(|_| "ssh".into())
}

/// A command that runs `script` on `host` inside a login shell (so the
/// user's PATH — nvm, Homebrew, cargo — is available).
pub fn command(host: &str, script: &str, interactive: bool) -> Command {
    let (program, args) = command_parts(host, script, interactive);
    let mut c = Command::new(program);
    c.args(args);
    c
}

/// Program and arguments of [`command`], for APIs that take them separately.
pub fn command_parts(host: &str, script: &str, interactive: bool) -> (String, Vec<String>) {
    let mut args = ssh_options(interactive);
    args.push(host.to_string());
    args.push("--".into());
    args.push(format!(
        "exec \"${{SHELL:-/bin/sh}}\" -lc {}",
        quote(script)
    ));
    (ssh_program(), args)
}

/// `cd <dir> && <cmd args…>` as a remote script.
pub fn script_in(dir: &str, program: &str, args: &[&str]) -> String {
    let mut s = format!("cd {} && {}", quote_path(dir), quote(program));
    for a in args {
        s.push(' ');
        s.push_str(&quote(a));
    }
    s
}

/// Program + args for a local PTY that opens an interactive shell (or runs
/// `program`) in `dir` on `host`.
pub fn pty_command(
    host: &str,
    dir: &str,
    program: Option<(&str, &[String])>,
) -> (String, Vec<String>) {
    let script = match program {
        Some((p, args)) => {
            let a: Vec<&str> = args.iter().map(String::as_str).collect();
            format!("{}; exec \"${{SHELL:-/bin/sh}}\" -l", script_in(dir, p, &a))
        }
        None => format!("cd {} && exec \"${{SHELL:-/bin/sh}}\" -l", quote_path(dir)),
    };
    let mut args = ssh_options(true);
    args.push(host.to_string());
    args.push("--".into());
    args.push(script);
    (ssh_program(), args)
}

/// Run a script on the host and return stdout (capped), failing on non-zero exit.
pub fn run(host: &str, script: &str, cap: usize) -> Result<String> {
    let mut child = command(host, script, false)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn {}", ssh_program()))?;
    let mut out = Vec::new();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let err_t = std::thread::spawn(move || {
        let mut e = String::new();
        let _ = (&mut stderr).take(64 * 1024).read_to_string(&mut e);
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        e
    });
    (&mut stdout).take(cap as u64).read_to_end(&mut out)?;
    let _ = std::io::copy(&mut stdout, &mut std::io::sink());
    let err = err_t.join().unwrap_or_default();
    if !child.wait()?.success() {
        bail!("{host}: {}", err.trim());
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Read a file, local or remote.
pub fn read_file(path: &Path) -> std::io::Result<Vec<u8>> {
    match split(path) {
        None => std::fs::read(path),
        Some((host, p)) => run(&host, &format!("cat {}", quote_path(&p)), 64 << 20)
            .map(String::into_bytes)
            .map_err(|e| std::io::Error::other(e.to_string())),
    }
}

pub fn read_to_string(path: &Path) -> std::io::Result<String> {
    read_file(path).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Write a file, local or remote (creating parent directories remotely).
pub fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    match split(path) {
        None => std::fs::write(path, bytes),
        Some((host, p)) => {
            let dir = p.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
            let script = format!("mkdir -p {} && cat > {}", quote_path(dir), quote_path(&p));
            let mut child = command(&host, &script, false)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()?;
            child.stdin.take().unwrap().write_all(bytes)?;
            let out = child.wait_with_output()?;
            if out.status.success() {
                Ok(())
            } else {
                Err(std::io::Error::other(
                    String::from_utf8_lossy(&out.stderr).trim().to_string(),
                ))
            }
        }
    }
}

pub fn exists(path: &Path) -> bool {
    match split(path) {
        None => path.exists(),
        Some((host, p)) => run(&host, &format!("test -e {}", quote_path(&p)), 1024).is_ok(),
    }
}

pub fn create_dir_all(path: &Path) -> std::io::Result<()> {
    match split(path) {
        None => std::fs::create_dir_all(path),
        Some((host, p)) => run(&host, &format!("mkdir -p {}", quote_path(&p)), 1024)
            .map(|_| ())
            .map_err(|e| std::io::Error::other(e.to_string())),
    }
}

/// Directory entries `(name, is_dir)`, hidden entries skipped, dirs first.
pub fn list_dir(path: &Path) -> Vec<(String, bool)> {
    let mut v: Vec<(String, bool)> = match split(path) {
        None => std::fs::read_dir(path)
            .map(|rd| {
                rd.flatten()
                    .map(|e| {
                        (
                            e.file_name().to_string_lossy().into_owned(),
                            e.file_type().is_ok_and(|t| t.is_dir()),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Some((host, p)) => run(&host, &format!("cd {} && ls -1Ap", quote_path(&p)), 4 << 20)
            .map(|s| {
                s.lines()
                    .map(|l| (l.trim_end_matches('/').to_string(), l.ends_with('/')))
                    .collect()
            })
            .unwrap_or_default(),
    };
    v.retain(|(n, _)| {
        !n.starts_with('.') && !["node_modules", "target", "dist", "build"].contains(&n.as_str())
    });
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths() {
        let p = join("dev@box", "/home/dev/app");
        assert_eq!(p, PathBuf::from("ssh://dev@box/home/dev/app"));
        assert_eq!(
            split(&p.join("src/a.rs")),
            Some(("dev@box".into(), "/home/dev/app/src/a.rs".into()))
        );
        assert_eq!(
            parse_target("box:/srv/repo"),
            Some(PathBuf::from("ssh://box/srv/repo"))
        );
        assert_eq!(
            parse_target("box:repo"),
            Some(PathBuf::from("ssh://box/~/repo"))
        );
        assert_eq!(split(Path::new("/local/path")), None);
        assert_eq!(quote("it's"), "'it'\\''s'");
        assert_eq!(quote("src/a.rs"), "src/a.rs");
        assert_eq!(quote_path("~/my repo"), "~/'my repo'");
        assert!(
            script_in("/r", "git", &["log", "--format=%H x"]).ends_with("git log '--format=%H x'")
        );
    }
}
