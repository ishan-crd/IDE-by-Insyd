//! End-to-end test of the SSH layer with a fake `ssh` that runs the remote
//! script locally. Exercises quoting, login-shell wrapping, git, file IO,
//! directory listing and the brain's streaming `cat-file` over "SSH".
#![cfg(unix)]

use insyde_core::{brain::Brain, git, project::Project, remote};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn sh(dir: &Path, cmd: &str) {
    let ok = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "{cmd}");
}

fn setup() -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("insyde-remote-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let repo = base.join("my repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    // Fake ssh: drop options and host, run the script with sh.
    let fake = base.join("fake-ssh");
    std::fs::write(
        &fake,
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -o) shift 2;; -t|-T) shift;; --) shift; break;; *) shift;; esac; done\nexec /bin/sh -c \"$*\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        repo.join("src/lib.rs"),
        "//! Demo crate.\npub fn hello() {}\n",
    )
    .unwrap();
    std::fs::write(repo.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    sh(
        &repo,
        "git init -q -b main && git -c user.email=t@t -c user.name=t add -A && git -c user.email=t@t -c user.name=t commit -qm 'init: switch to fake ssh'",
    );
    (repo, fake)
}

#[test]
fn ssh_layer_end_to_end() {
    let (repo, fake) = setup();
    let repo = std::fs::canonicalize(&repo).unwrap();
    // SAFETY: this test binary runs this single test; nothing reads the env concurrently.
    unsafe { std::env::set_var("INSYDE_SSH", &fake) };
    let target = remote::parse_target(&format!("devbox:{}", repo.display())).unwrap();
    assert!(remote::is_remote(&target));

    // Probe → root keeps the host.
    let row = Project::probe(&target).unwrap();
    assert_eq!(row.path, target);
    assert_eq!(row.base, "main");

    // Worktrees come back as ssh:// paths; create one remotely.
    let wts = git::list_worktrees(&target).unwrap();
    assert_eq!(wts[0].path, target);
    let wt = git::add_worktree(&target, "feat/remote-test", "main").unwrap();
    assert!(remote::is_remote(&wt));
    assert!(remote::exists(&wt.join("src/lib.rs")));

    // File IO through the remote layer (paths with spaces and quotes).
    let f = wt.join("src/it's new.rs");
    remote::write_file(&f, b"pub fn x() {}\n").unwrap();
    assert_eq!(remote::read_to_string(&f).unwrap(), "pub fn x() {}\n");
    let names: Vec<String> = remote::list_dir(&wt.join("src"))
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert!(names.contains(&"it's new.rs".to_string()), "{names:?}");

    // Changes are seen by git over the same layer.
    let changed = git::changed_files(&wt, "main");
    assert!(
        changed.iter().any(|c| c.path == "src/it's new.rs"),
        "{changed:?}"
    );

    // Run-command detection works remotely.
    assert_eq!(
        insyde_core::project::run_command(&wt).map(|r| r.0),
        Some("cargo run".into())
    );

    // The brain streams blobs with `git cat-file --batch` over "SSH".
    let brain = Brain::open(&target, "main").unwrap();
    let g = brain.build(std::sync::Arc::new(|_, _| {})).unwrap();
    assert!(
        g.nodes
            .iter()
            .any(|n| n.name == "lib.rs" && n.summary.contains("Demo crate")),
        "{:?}",
        g.nodes.iter().map(|n| &n.name).collect::<Vec<_>>()
    );

    git::remove_worktree(&target, &wt, true).unwrap();
}
