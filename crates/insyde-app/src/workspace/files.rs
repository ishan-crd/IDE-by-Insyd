//! File tree right-click menu: open (panel, new tab, default app), reveal,
//! copy, copy path, rename, duplicate, new file/folder, move to Trash.

use super::{AgentTab, FileMenu, NameEdit, TabView, Workspace};
use crate::ui;
use gpui::prelude::*;
use gpui::{AnyElement, ClipboardItem, Context, SharedString, Window, anchored, div, px};
use gpui_component::input::{InputEvent, InputState};
use insyde_core::fileops;
use insyde_theme::{Theme, metrics};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
enum Action {
    Open,
    OpenTab,
    OpenCenter,
    Reveal,
    Terminal,
    Copy,
    CopyPath,
    CopyRel,
    Rename,
    Duplicate,
    NewFile,
    NewFolder,
    Trash,
}

impl Workspace {
    pub fn open_file_menu(
        &mut self,
        path: PathBuf,
        rel: String,
        is_dir: bool,
        pos: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.run_menu = None;
        self.file_menu = Some(FileMenu {
            path,
            rel,
            is_dir,
            pos,
            edit: None,
        });
        cx.notify();
    }

    /// Open `rel` as a tab in the center strip (or focus it if already open).
    pub fn open_file_tab(&mut self, rel: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.active_wt_path() else {
            return;
        };
        if let Some(ws) = self.wt_mut()
            && let Some(i) = ws
                .tabs
                .iter()
                .position(|t| matches!(&t.view, TabView::File(e) if e.read(cx).rel == rel))
        {
            ws.active = i;
            cx.notify();
            return;
        }
        let ed = cx.new(|cx| crate::editor::FileEditor::open(root, rel, None, window, cx));
        self._subs.push(cx.subscribe(
            &ed,
            |this, _, ev: &crate::editor::EditorEvent, cx| match ev {
                crate::editor::EditorEvent::Saved(p) => {
                    this.log(format!("Saved {p}"));
                    this.refresh_wt_details(cx);
                }
                crate::editor::EditorEvent::Error(e) => this.toast(e.clone(), true, cx),
            },
        ));
        let id = self.next_id();
        let agent = insyde_core::agents::AGENTS[Self::default_agent_index()].id;
        if let Some(ws) = self.wt_mut() {
            ws.tabs.push(AgentTab {
                id,
                agent,
                view: TabView::File(ed),
            });
            ws.active = ws.tabs.len() - 1;
        }
        cx.notify();
    }

    fn file_action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let Some(m) = &self.file_menu else { return };
        let (path, rel, is_dir) = (m.path.clone(), m.rel.clone(), m.is_dir);
        if rel.is_empty() && matches!(action, Action::Rename | Action::Duplicate | Action::Trash) {
            return;
        }
        match action {
            Action::Rename => return self.start_name_edit(NameEdit::Rename, window, cx),
            Action::NewFile => return self.start_name_edit(NameEdit::NewFile, window, cx),
            Action::NewFolder => return self.start_name_edit(NameEdit::NewFolder, window, cx),
            _ => {}
        }
        self.file_menu = None;
        match action {
            Action::Open => self.open_file(rel, None, window, cx),
            Action::OpenTab => self.open_file_in(rel, None, true, window, cx),
            Action::OpenCenter => self.open_file_tab(rel, window, cx),
            Action::Reveal => self.report(fileops::reveal(&path), None, cx),
            Action::Terminal => {
                let dir = if is_dir {
                    path.clone()
                } else {
                    path.parent().map(Path::to_path_buf).unwrap_or(path.clone())
                };
                let arg = insyde_core::remote::quote_path(&insyde_core::remote::arg(&dir));
                self.add_pane(Some(format!("cd {arg}")), cx);
            }
            Action::Copy => {
                let name = file_name(&path);
                self.report(
                    copy_file_to_pasteboard(&path),
                    Some(format!("Copied {name}")),
                    cx,
                );
            }
            Action::CopyPath => {
                let p = insyde_core::remote::arg(&path);
                cx.write_to_clipboard(ClipboardItem::new_string(p.clone()));
                self.toast(format!("Copied {p}"), false, cx);
            }
            Action::CopyRel => {
                cx.write_to_clipboard(ClipboardItem::new_string(rel.clone()));
                self.toast(format!("Copied {rel}"), false, cx);
            }
            Action::Duplicate => {
                self.fs_task(
                    window,
                    cx,
                    move || fileops::duplicate(&path),
                    |this, to, _, cx| {
                        this.toast(format!("Created {}", file_name(&to)), false, cx);
                    },
                );
            }
            Action::Trash => {
                let remote = insyde_core::remote::is_remote(&path);
                self.fs_task(
                    window,
                    cx,
                    move || fileops::trash(&path),
                    move |this, _, _, cx| {
                        this.forget_path(&rel, cx);
                        let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
                        this.toast(
                            if remote {
                                format!("Deleted {name}")
                            } else {
                                format!("Moved {name} to the Trash")
                            },
                            false,
                            cx,
                        );
                    },
                );
            }
            Action::Rename | Action::NewFile | Action::NewFolder => {}
        }
        cx.notify();
    }

    fn report(&mut self, res: anyhow::Result<()>, ok: Option<String>, cx: &mut Context<Self>) {
        match res {
            Ok(()) => {
                if let Some(msg) = ok {
                    self.toast(msg, false, cx);
                }
            }
            Err(e) => self.toast(format!("{e:#}"), true, cx),
        }
    }

    /// Run a file operation off the UI thread (remote ones are round-trips),
    /// then refresh the tree and diff.
    fn fs_task<T: Send + 'static>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        op: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let task = cx.background_spawn(async move { op() });
        cx.spawn_in(window, async move |this, cx| {
            let res = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match res {
                    Ok(v) => done(this, v, window, cx),
                    Err(e) => this.toast(format!("{e:#}"), true, cx),
                }
                this.tree_cache = None;
                this.refresh_wt_details(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn start_name_edit(&mut self, kind: NameEdit, window: &mut Window, cx: &mut Context<Self>) {
        let Some(m) = &self.file_menu else { return };
        let current = if kind == NameEdit::Rename {
            file_name(&m.path)
        } else {
            String::new()
        };
        let placeholder = match kind {
            NameEdit::Rename => "New name",
            NameEdit::NewFile => "File name, e.g. notes.md",
            NameEdit::NewFolder => "Folder name",
        };
        let input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder(placeholder);
            st.set_value(current, window, cx);
            st
        });
        self._subs.push(
            cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.commit_name_edit(window, cx);
                }
            }),
        );
        input.update(cx, |st, cx| st.focus(window, cx));
        if let Some(m) = &mut self.file_menu {
            m.edit = Some((kind, input));
        }
        cx.notify();
    }

    fn commit_name_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(FileMenu {
            path,
            rel,
            is_dir,
            edit: Some((kind, input)),
            ..
        }) = self.file_menu.as_ref()
        else {
            return;
        };
        let name = input.read(cx).value().trim().to_string();
        if !fileops::valid_name(&name) {
            self.toast("Use a name without slashes", true, cx);
            return;
        }
        let (path, rel, is_dir, kind) = (path.clone(), rel.clone(), *is_dir, *kind);
        self.file_menu = None;
        let rel_dir = |rel: &str| rel.rsplit_once('/').map(|(d, _)| d.to_string());
        let join_rel = |dir: Option<String>, name: &str| match dir {
            Some(d) if !d.is_empty() => format!("{d}/{name}"),
            _ => name.to_string(),
        };
        match kind {
            NameEdit::Rename => {
                let to = path.with_file_name(&name);
                if to == path {
                    return;
                }
                let to_rel = join_rel(rel_dir(&rel), &name);
                let from = path.clone();
                self.fs_task(
                    window,
                    cx,
                    move || fileops::rename(&from, &to),
                    move |this, _, _, cx| this.moved_path(&path, &rel, &to_rel, cx),
                );
            }
            NameEdit::NewFile | NameEdit::NewFolder => {
                // New items go inside a folder, or next to a file.
                let (dir, dir_rel) = if is_dir {
                    (path.clone(), Some(rel.clone()))
                } else {
                    (
                        path.parent().map(Path::to_path_buf).unwrap_or(path.clone()),
                        rel_dir(&rel),
                    )
                };
                let target = dir.join(&name);
                let new_rel = join_rel(dir_rel, &name);
                self.tree_open.insert(dir);
                if kind == NameEdit::NewFile {
                    self.fs_task(
                        window,
                        cx,
                        move || fileops::create_file(&target),
                        move |this, _, window, cx| this.open_file(new_rel, None, window, cx),
                    );
                } else {
                    self.fs_task(
                        window,
                        cx,
                        move || fileops::create_dir(&target),
                        |_, _, _, _| {},
                    );
                }
            }
        }
        cx.notify();
    }

    /// Point open editors and expanded folders at a renamed path.
    fn moved_path(&mut self, from: &Path, from_rel: &str, to_rel: &str, cx: &mut Context<Self>) {
        let to = from.with_file_name(to_rel.rsplit('/').next().unwrap_or(to_rel));
        self.tree_open = self
            .tree_open
            .iter()
            .map(|p| match p.strip_prefix(from) {
                Ok(rest) => to.join(rest),
                Err(_) => p.clone(),
            })
            .collect();
        for ed in self.open_editors() {
            ed.update(cx, |e, cx| {
                if let Some(rest) = under(&e.rel, from_rel) {
                    e.rel = format!("{to_rel}{rest}");
                    cx.notify();
                }
            });
        }
    }

    /// Close editors showing a path that no longer exists.
    fn forget_path(&mut self, rel: &str, cx: &mut Context<Self>) {
        let gone = |e: &crate::editor::FileEditor| under(&e.rel, rel).is_some();
        if let Some(ws) = self.wt_mut() {
            ws.editors.retain(|e| !gone(e.read(cx)));
            ws.editor_ix = ws.editor_ix.min(ws.editors.len().saturating_sub(1));
            let before = ws.tabs.len();
            ws.tabs
                .retain(|t| !matches!(&t.view, TabView::File(e) if gone(e.read(cx))));
            if ws.tabs.len() != before {
                ws.active = ws.active.min(ws.tabs.len().saturating_sub(1));
            }
        }
    }

    fn open_editors(&self) -> Vec<gpui::Entity<crate::editor::FileEditor>> {
        let Some(ws) = self.wt() else { return vec![] };
        ws.editors
            .iter()
            .cloned()
            .chain(ws.tabs.iter().filter_map(|t| match &t.view {
                TabView::File(e) => Some(e.clone()),
                _ => None,
            }))
            .collect()
    }

    pub(super) fn render_file_menu(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(m) = &self.file_menu else {
            return div().into_any_element();
        };
        let remote = insyde_core::remote::is_remote(&m.path);
        let is_root = m.rel.is_empty();
        let mut menu = div()
            .w(px(232.))
            .p(px(4.))
            .bg(t.panel)
            .border_1()
            .border_color(t.line)
            .rounded(metrics::RADIUS_LG)
            .shadow(t.pop_shadow(true))
            .occlude()
            .flex()
            .flex_col()
            .text_size(metrics::TEXT_SM);
        menu = menu.child(
            ui::trunc(SharedString::from(file_name(&m.path)))
                .px(px(8.))
                .pt(px(5.))
                .pb(px(4.))
                .text_size(metrics::TEXT_XS)
                .text_color(t.ink_3),
        );
        if let Some((kind, input)) = &m.edit {
            let hint = match kind {
                NameEdit::Rename => "Enter to rename · Esc to cancel",
                NameEdit::NewFile => "Enter to create the file",
                NameEdit::NewFolder => "Enter to create the folder",
            };
            menu = menu.child(
                div()
                    .p(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(gpui_component::input::Input::new(input).h(px(28.)))
                    .child(
                        div()
                            .px(px(4.))
                            .text_size(metrics::TEXT_XS)
                            .text_color(t.ink_3)
                            .child(hint),
                    ),
            );
        } else {
            let groups: Vec<Vec<(Action, &str)>> = if m.is_dir {
                vec![
                    vec![
                        (Action::NewFile, "New File…"),
                        (Action::NewFolder, "New Folder…"),
                    ],
                    vec![
                        (Action::Reveal, "Reveal in Finder"),
                        (Action::Terminal, "Open in Terminal"),
                    ],
                    vec![
                        (Action::Copy, "Copy"),
                        (Action::CopyPath, "Copy Path"),
                        (Action::CopyRel, "Copy Relative Path"),
                    ],
                    vec![
                        (Action::Rename, "Rename…"),
                        (Action::Duplicate, "Duplicate"),
                    ],
                    vec![(Action::Trash, "Move to Trash")],
                ]
            } else {
                vec![
                    vec![
                        (Action::Open, "Open"),
                        (Action::OpenTab, "Open in New Tab"),
                        (Action::OpenCenter, "Open in Agent Area"),
                    ],
                    vec![
                        (Action::Reveal, "Reveal in Finder"),
                        (Action::Terminal, "Open in Terminal"),
                    ],
                    vec![
                        (Action::Copy, "Copy"),
                        (Action::CopyPath, "Copy Path"),
                        (Action::CopyRel, "Copy Relative Path"),
                    ],
                    vec![
                        (Action::Rename, "Rename…"),
                        (Action::Duplicate, "Duplicate"),
                    ],
                    vec![(Action::Trash, "Move to Trash")],
                ]
            };
            let mut first = true;
            for group in groups {
                // Finder, pasteboard and default-app actions need a local file.
                let group: Vec<_> = group
                    .into_iter()
                    .filter(|(a, ..)| {
                        !(remote && matches!(a, Action::Reveal | Action::Copy))
                            // The worktree root itself can't be renamed or trashed here.
                            && !(is_root
                                && matches!(
                                    a,
                                    Action::Rename
                                        | Action::Duplicate
                                        | Action::Trash
                                        | Action::CopyRel
                                ))
                    })
                    .collect();
                if group.is_empty() {
                    continue;
                }
                if !first {
                    menu = menu.child(div().my(px(4.)).mx(px(8.)).h(px(1.)).bg(t.line_soft));
                }
                first = false;
                for (action, label) in group {
                    let danger = matches!(action, Action::Trash);
                    let label = if danger && remote {
                        "Delete permanently"
                    } else {
                        label
                    };
                    let hover = if danger { t.err_bg } else { t.hover };
                    menu = menu.child(
                        div()
                            .id(SharedString::from(format!("fm-{label}")))
                            .flex()
                            .items_center()
                            .h(px(28.))
                            .px(px(8.))
                            .rounded(metrics::RADIUS)
                            .cursor_pointer()
                            .text_color(if danger { t.err } else { t.ink })
                            .hover(move |s| s.bg(hover))
                            .child(label)
                            .on_click(
                                cx.listener(move |this, _, w, cx| this.file_action(action, w, cx)),
                            ),
                    );
                }
            }
        }
        anchored()
            .position(m.pos)
            .snap_to_window_with_margin(px(8.))
            .child(menu)
            .into_any_element()
    }
}

/// Put the file itself on the pasteboard, so Finder, Mail or Slack can paste it.
#[cfg(target_os = "macos")]
fn copy_file_to_pasteboard(path: &Path) -> anyhow::Result<()> {
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPasteboard, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let item: objc2::rc::Retained<ProtocolObject<dyn NSPasteboardWriting>> =
        ProtocolObject::from_retained(url);
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    if pb.writeObjects(&NSArray::from_retained_slice(&[item])) {
        Ok(())
    } else {
        anyhow::bail!("couldn't copy {}", path.display())
    }
}

#[cfg(not(target_os = "macos"))]
fn copy_file_to_pasteboard(path: &Path) -> anyhow::Result<()> {
    anyhow::bail!("copying files isn't supported here ({})", path.display())
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The rest of `rel` after `base` if `rel` is `base` or inside it.
fn under<'a>(rel: &'a str, base: &str) -> Option<&'a str> {
    let rest = rel.strip_prefix(base)?;
    (rest.is_empty() || rest.starts_with('/')).then_some(rest)
}
