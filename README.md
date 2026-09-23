# InsyDE by Insyd

A native, GPU-rendered IDE for AI coding agents, written in Rust on GPUI (Zed's UI framework).
Every task gets its own git worktree, terminal and agent sessions; a **Project Brain** gives every
agent persistent context about the repository's main branch.

![design](design/InsyDE.dc.html)

## What works today

- **Worktrees**: projects and their git worktrees in the sidebar (diff stats, PR number, live/warn
  status), create a task worktree from a title in ~0.1 s, remove with a guard, swipe between projects.
- **Agents over ACP** (Agent Client Protocol): Claude Code, Codex, OpenCode, Pi, Grok, Cursor,
  Antigravity as chat sessions, or any of them as their own terminal UI (hold ⌘ when picking).
  Streaming markdown, tool-call cards, plans, inline permission prompts, model and approval-policy
  pickers, context-window meter, cost. Sessions persist in SQLite and resume with `session/load`.
  Agent processes start lazily on the first message, so restored tabs cost no memory.
- **Hand off**: start a fresh agent in the same worktree carrying the conversation summary, the
  worktree diff, terminal output, open plan items and brain notes (token estimate per part).
- **Project Brain**: built from `main` without a checkout (one `ls-tree`, one `cat-file --batch`,
  one `git log`; ~1.3 s on a 1,300-node repo). Feature areas, files, exported symbols, import
  links, API routes, merged PRs, decision commits, docs and inferred conventions, stored like
  SiYuan (typed nodes, refs, FTS5). Graph view, area tree, inspector, pins ("Always include in
  agent context"), SiYuan-style notes with `[[links]]`. Agents get a token-budgeted digest.
- **Terminals**: GPU-painted `alacritty_terminal` panes with split, resize, pop-out windows,
  scrollback, xterm key handling, bracketed paste; Logs and Problems tabs.
- **Review**: PR checks via `gh` (re-run failed), changed files with per-file diff, file viewer,
  content search, one-click draft PR.
- Light and dark themes from the design tokens; all regions resizable and persisted.

## Build and run (macOS)

```sh
rustup toolchain install 1.98.1        # pinned in rust-toolchain.toml
cargo run -p insyde-app --release      # binary: target/release/insyde
```

Agents run under your own logins: have `node` (for npx-based adapters), and the CLIs you want
(`claude`, `codex`, `opencode`, …) installed and signed in. PRs and checks use the `gh` CLI.

Faster startup: install Xcode's Metal toolchain and precompile shaders.

```sh
xcodebuild -downloadComponent MetalToolchain
cargo build -p insyde-app --release --no-default-features
```

## Layout

```
crates/insyde-theme   design tokens (light/dark) from design/InsyDE.dc.html
crates/insyde-core    git, worktrees, gh, SQLite store, PTY terminals, ACP client, Project Brain
crates/insyde-app     GPUI app: workspace, chat, terminal view, brain view
docs/                 research, stack, architecture, plan, status
```

Shortcuts: ⌘T new agent · ⌘W close tab · ⌘B sidebar · ⌘J terminals · ⌥⌘B right panel ·
⇧⌘L theme · ⌘O open repository · ctrl-` new terminal · 1–9 in the agent picker.
