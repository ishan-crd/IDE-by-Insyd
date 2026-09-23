# Status against the plan (2026-09-23)

Crates are named `insyde-*` (the plan's `ide-*`).

| Phase | State | Notes |
|---|---|---|
| 0 Bootstrap | Done | Workspace, pinned toolchain, theme tokens from the design. CI not set up yet. |
| 1 Shell, layout | Done | Design layout, resizable + persisted regions, light/dark. No command palette yet. |
| 2 Worktrees | Done | List/create/remove, stats, PR badges, per-worktree state, swipe between projects. |
| 3 Terminal | Done | alacritty grid, keys, scrollback, split, pop-out, mouse selection and copy. OSC 133 prompt marks not yet. |
| 4 Agents (ACP) | Done (core) | 7 ACP agents + TUI mode, permissions, persistence, resume, hand-off. |
| 5 Files | Done | File tree, search, code editor (tree-sitter highlighting, line numbers, folding, find), ⌘S save, revert, reload after agent edits without clobbering unsaved work. |
| 6 Review | Done (core) | Checks, re-run, draft PR, line-numbered diff, review comments on any diff line (stored per worktree) sent to the agent as one prompt. Merge button still to do. |
| 7 CLI / socket | Done | `insy` CLI over a 0600 Unix socket (JSON-RPC): status, worktrees, agents new/send/wait/read, brain search, open, team state. Terminals and agents get `INSYDE_SOCKET` and `insy` on PATH. PiP: terminal pop-out windows. Browser tab opens the system browser. |
| 8 Teams | Done | Lead + specialists from the agent menu ("Team…") or `insy team run team.toml`; specialists get their own worktrees and role prompts; coordination via `insy` (pure `insy` shell commands are auto-approved); Team tab shows members and shared state. |
| 9 Remote | Done | Open `user@host:/path` (sidebar folder button or `insy open`). Git, worktrees, terminals (`ssh -t`), agents (ACP over the SSH pipe), GitHub, the editor, search, the file tree and the brain all run on the host through one multiplexed connection. Tested end to end with a fake `ssh` (`INSYDE_SSH`). |
| 10 Packaging | Done (macOS) | `packaging/macos/bundle.sh` builds `InsyDE.app` (app + `insy`, icon from the design mark, Info.plist), signs it (ad-hoc, or `SIGN_IDENTITY`), and makes a `.dmg` (~10 MB). Tag `v*` to publish a release from CI. Notarization needs a Developer ID. Windows/Linux builds not done. |
| Settings (new) | Done | Typed `settings.json` read by core and app; settings screen with categories, search, changed-only filter and per-setting reset; ⌘, opens it. Glass mode: blurred translucent chrome with floating sheets, adjustable tint, live toggle. |
| Web access (new) | Done | Browser client in the InsyDE theme (projects and threads, streaming chat with approvals, model/mode/policy pickers, new thread in a new worktree, changes panel with diffs, commit/push/PR, terminal drawer with a phone key bar, command palette). Served from the app (Settings › Web access: this Mac, LAN + Tailscale, or a cloudflared public link; QR pairing) or `insy serve`. Web threads run in the server's hub, separate from the desktop's open tabs (they share history). |
| Brain (new) | Done | SiYuan-style model, graph, notes, digest injected into agent prompts. |

## Deviations from docs/02-STACK.md

- **git**: system `git` for reads too (not `gix`). Every call is bounded (8 concurrent, output
  capped and drained); brain indexing uses streaming `cat-file --batch`. Revisit if status gets hot.
- **No tokio**: the ACP SDK is runtime-agnostic; each agent session runs on a small `smol` thread,
  UI work on GPUI's executors.
- **objc2**: `objc2-foundation` (core, NSFileManager trash) and `objc2-app-kit` (app, file
  pasteboard) are named directly; both were already in the tree through gpui.
- **Shaders**: default feature `runtime-shaders` compiles Metal shaders at launch (~0.8 s) because
  the Metal toolchain isn't installed by default in Xcode 26. See README for the fast path.
- **Top bar layout**: three columns sized to the panes below (brand over the sidebar; Brain,
  agent count, history and Hand off over the agent area; cost, layout, theme and Run over the right
  panel), so each group follows its pane when resized. The design's context meter is omitted
  (user request); Hand off / history popovers anchor under their buttons.
- **File tree context menu**: right-click a file or folder for Open, Open in New Tab (another
  tab in the right panel's Editor), Open in Agent Area (a file tab in the center strip), Reveal in Finder, Open in Terminal, Copy (the file
  itself), Copy Path, Copy Relative Path, Rename, Duplicate, New File/Folder, Move to Trash
  (`insyde-core::fileops`; over SSH, Delete permanently). Empty space opens the root's menu.
- **Top bar right side**: no Create PR button (the Checks panel still has it); the Run split
  button takes its place. Run is labelled by the detected tool (`Run pnpm`, `Run cargo`), can be
  overridden in Settings > Terminal, and its menu runs saved quick commands or any typed one.

## Measured (M-series Mac, release)

| Metric | Value | Budget |
|---|---|---|
| Idle RSS, one worktree, terminal + restored chat | 92 MB | < 150 MB |
| Window visible (runtime shaders) | ~1.5 s | < 0.4 s (needs precompiled shaders + init work) |
| Brain build, t3code (1,313 nodes) | 1.3 s | — |
| Worktree create | ~0.1 s | < 0.1 s UI |
| Binary size | 16 MB | — |
