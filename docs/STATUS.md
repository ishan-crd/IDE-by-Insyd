# Status against the plan (2026-09-23)

Crates are named `insyde-*` (the plan's `ide-*`).

| Phase | State | Notes |
|---|---|---|
| 0 Bootstrap | Done | Workspace, pinned toolchain, theme tokens from the design. CI not set up yet. |
| 1 Shell, layout | Done | Design layout, resizable + persisted regions, light/dark. No command palette yet. |
| 2 Worktrees | Done | List/create/remove, stats, PR badges, per-worktree state, swipe between projects. |
| 3 Terminal | Done (core) | alacritty grid, keys, scrollback, split, pop-out. Missing: mouse selection, OSC 133. |
| 4 Agents (ACP) | Done (core) | 7 ACP agents + TUI mode, permissions, persistence, resume, hand-off. |
| 5 Files | Done | File tree, search, code editor (tree-sitter highlighting, line numbers, folding, find), ⌘S save, revert, reload after agent edits without clobbering unsaved work. |
| 6 Review | Partial | Checks, diff, draft PR, re-run. Missing: line comments → agent, merge button. |
| 7 CLI / socket | Not started | |
| 8 Teams | Not started | "Super" is listed; runs Claude Code today. |
| 9 Remote | Not started | |
| 10 Packaging | Not started | |
| Brain (new) | Done | SiYuan-style model, graph, notes, digest injected into agent prompts. |

## Deviations from docs/02-STACK.md

- **git**: system `git` for reads too (not `gix`). Every call is bounded (8 concurrent, output
  capped and drained); brain indexing uses streaming `cat-file --batch`. Revisit if status gets hot.
- **No tokio**: the ACP SDK is runtime-agnostic; each agent session runs on a small `smol` thread,
  UI work on GPUI's executors.
- **Shaders**: default feature `runtime-shaders` compiles Metal shaders at launch (~0.8 s) because
  the Metal toolchain isn't installed by default in Xcode 26. See README for the fast path.

## Measured (M-series Mac, release)

| Metric | Value | Budget |
|---|---|---|
| Idle RSS, one worktree, terminal + restored chat | 92 MB | < 150 MB |
| Window visible (runtime shaders) | ~1.5 s | < 0.4 s (needs precompiled shaders + init work) |
| Brain build, t3code (1,313 nodes) | 1.3 s | — |
| Worktree create | ~0.1 s | < 0.1 s UI |
| Binary size | 16 MB | — |
