//! File-tree actions (rename, duplicate, trash, new file/folder, reveal), for
//! local and SSH worktrees. Blocking; call off the UI thread for remote paths.

use crate::remote::{self, quote_path};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

fn remote_run(host: &str, script: &str) -> Result<()> {
    remote::run(host, script, 64 * 1024).map(|_| ())
}

/// Move to the Trash locally (recoverable); delete outright over SSH.
pub fn trash(path: &Path) -> Result<()> {
    match remote::split(path) {
        Some((host, p)) => remote_run(&host, &format!("rm -rf -- {}", quote_path(&p))),
        None => trash_local(path),
    }
}

#[cfg(target_os = "macos")]
fn trash_local(path: &Path) -> Result<()> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|e| anyhow::anyhow!("{}", e.localizedDescription()))
}

#[cfg(not(target_os = "macos"))]
fn trash_local(path: &Path) -> Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Rename or move; refuses to overwrite.
pub fn rename(from: &Path, to: &Path) -> Result<()> {
    if remote::exists(to) {
        bail!("{} already exists", to.display());
    }
    match (remote::split(from), remote::split(to)) {
        (Some((host, a)), Some((_, b))) => remote_run(
            &host,
            &format!("mv -n -- {} {}", quote_path(&a), quote_path(&b)),
        ),
        _ => std::fs::rename(from, to).context("rename"),
    }
}

/// Copy next to the original as `name copy.ext` (`name copy 2.ext`, …).
pub fn duplicate(path: &Path) -> Result<PathBuf> {
    let to = copy_name(path, remote::exists);
    match (remote::split(path), remote::split(&to)) {
        (Some((host, a)), Some((_, b))) => remote_run(
            &host,
            &format!("cp -R -- {} {}", quote_path(&a), quote_path(&b)),
        )?,
        _ if path.is_dir() => copy_dir(path, &to)?,
        _ => {
            std::fs::copy(path, &to).context("copy")?;
        }
    }
    Ok(to)
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let dest = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &dest)?;
        } else {
            std::fs::copy(e.path(), dest)?;
        }
    }
    Ok(())
}

/// First free `stem copy[ n].ext` beside `path`.
fn copy_name(path: &Path, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let dir = path.parent().unwrap_or(Path::new(""));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Dotfiles and directories keep their whole name as the stem.
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !path.is_dir() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    (1..)
        .map(|n| {
            let suffix = if n == 1 {
                " copy".to_string()
            } else {
                format!(" copy {n}")
            };
            dir.join(format!("{stem}{suffix}{ext}"))
        })
        .find(|p| !exists(p))
        .expect("unbounded")
}

/// Create an empty file; refuses to overwrite.
pub fn create_file(path: &Path) -> Result<()> {
    if remote::exists(path) {
        bail!("{} already exists", path.display());
    }
    if let Some(dir) = path.parent() {
        remote::create_dir_all(dir)?;
    }
    remote::write_file(path, b"").context("create file")
}

pub fn create_dir(path: &Path) -> Result<()> {
    if remote::exists(path) {
        bail!("{} already exists", path.display());
    }
    remote::create_dir_all(path).context("create folder")
}

/// Select the item in Finder.
pub fn reveal(path: &Path) -> Result<()> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
        .context("open -R")?;
    Ok(())
}

/// A single path segment a user typed: no separators, not `.`/`..`, not empty.
pub fn valid_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && n != "." && n != ".." && !n.contains('/') && !n.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_names() {
        let none = |_: &Path| false;
        assert_eq!(
            copy_name(Path::new("/a/main.rs"), none),
            Path::new("/a/main copy.rs")
        );
        assert_eq!(
            copy_name(Path::new("/a/.env"), none),
            Path::new("/a/.env copy")
        );
        let taken = |p: &Path| p == Path::new("/a/x copy.txt");
        assert_eq!(
            copy_name(Path::new("/a/x.txt"), taken),
            Path::new("/a/x copy 2.txt")
        );
    }

    #[test]
    fn names() {
        assert!(valid_name("a.rs"));
        assert!(!valid_name("a/b"));
        assert!(!valid_name(".."));
        assert!(!valid_name("  "));
    }
}
