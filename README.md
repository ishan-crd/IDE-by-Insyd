# InsyDE by Insyd

A native, GPU-rendered IDE for AI coding agents, written in Rust on GPUI (Zed's UI framework).
Every task gets its own git worktree, terminal and agent sessions; a **Project Brain** gives every
agent persistent context about the repository's main branch.


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
- **Remote over SSH**: open `user@host:/path/to/repo` from the sidebar's folder button or
  `insy open`. Git, worktrees, terminals, agents, GitHub, the editor, search and the brain run on the
  host through one multiplexed SSH connection (uses your `~/.ssh/config` and keys).
- **Teams**: a lead plus specialists, each in its own worktree, coordinating through `insy`.
- **Editor and review**: code editor with highlighting and ⌘S; comment on any diff line and send
  the comments to the agent.
- Light and dark themes from the design tokens; all regions resizable and persisted.
- **Glass** (Settings, Look & feel): the sidebars, top bar and status bar turn translucent over a
  blurred desktop, and chats and terminals float on it as solid rounded sheets. Tint is adjustable
  and it switches live.
- **Settings** (gear in the sidebar or ⌘,): ten categories covering look and accent, agents
  (default agent, approval policy, launch-command overrides, extra environment), the brain,
  worktrees and git (location, branch prefix, setup command, files to copy, draft PRs), terminal,
  editor, notifications, privacy and data cleanup, shortcuts and About. Search across all of them,
  show only what you changed, reset any one. Everything saves to `settings.json` in the data folder,
  which you can also edit by hand and reload.

## Command line

`insy` (built next to the app) controls a running InsyDE; agents inside InsyDE can use it too.

```sh
insy status                         # project, worktree, open agents
insy worktree create "fix login"    # new task worktree
insy agent new codex --prompt "..." # start an agent (prints its id)
insy agent send 3 "run the tests" && insy agent wait 3
insy brain search "auth refresh"
insy coord set plan "..."           # shared state for agent teams
```

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

## Package

```sh
packaging/macos/bundle.sh     # target/release/bundle/InsyDE.app and InsyDE-<version>.dmg
ln -sf /Applications/InsyDE.app/Contents/MacOS/insy /usr/local/bin/insy   # CLI on PATH
```

Pushing a `v*` tag builds the `.dmg` in CI and attaches it to a GitHub release. Builds are
ad-hoc signed; set `SIGN_IDENTITY` to a Developer ID for distribution (and notarize).

## Layout

```
crates/insyde-theme   design tokens (light/dark) from design/InsyDE.dc.html
crates/insyde-core    git, worktrees, gh, SQLite store, PTY terminals, ACP client, Project Brain
crates/insyde-app     GPUI app: workspace, chat, terminal view, brain view, editor
crates/insyde-cli     `insy`, the command-line client for a running InsyDE
packaging/macos       app bundle, icon and dmg script
docs/                 research, stack, architecture, plan, status
```

Shortcuts: ⌘T new agent · ⌘W close tab · ⌘B sidebar · ⌘J terminals · ⌥⌘B right panel ·
⇧⌘L theme · ⌘O open repository · ctrl-` new terminal · 1–9 in the agent picker.
