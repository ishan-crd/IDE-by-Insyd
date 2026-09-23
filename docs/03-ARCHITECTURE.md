# 03 — Architecture

## 1. Process model

```
┌──────────────────────────────── ide-app (GPUI, main thread = UI) ────────────────────────────────┐
│  Views (ide-ui): Shell, Sidebar, Dock, ChatView, TerminalView, DiffView, EditorTab, Palette...    │
│        ▲ Entity<T> state, cx.observe / cx.subscribe                                               │
│        │ events                                     commands ▼                                   │
│  ┌─────┴──────────────────────────────── CoreHandle (in-process) ────────────────────────────┐   │
│  │  ide-core (tokio runtime, N worker threads): Workspaces, Worktrees, Sessions, Terminals,  │   │
│  │  Providers (ACP/Codex/Claude/PTY), Git, Forge, Search, Watcher, Store (SQLite), Socket    │   │
│  └───────────────────────────────────────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
        ▲ unix socket / named pipe, JSON-RPC 2.0 (same request/event types as CoreHandle)
        │
   ide (CLI)          ide-server (Phase 9, headless core on a remote host, over SSH)
```

- `ide-core` is a library with **no GPUI dependency**. Its public API is a set of `serde` request/response/event types (`ide-proto`) and a `CoreHandle { request(...) -> Result<Response>, events() -> Stream<Event> }`.
- The GPUI app links `ide-core` directly (in-process) for zero-latency; the CLI and remote clients use the identical API over a socket. This is what lets `ide worktree create ...` or `ide agent send ...` work from a terminal inside the app, and lets teams/orchestration be scripted.
- One tokio runtime lives in `ide-core`; UI-side futures use GPUI executors. Crossing the boundary is always a channel, never a blocking call on the main thread.

## 2. Crate layout (Cargo workspace)

```
Cargo.toml                 # workspace, [workspace.dependencies] pins everything
rust-toolchain.toml
crates/
  ide-proto/               # serde types: Request, Response, Event, ids, models. schemars for JSON schema.
  ide-core/                # the daemon core (tokio). Modules below.
    src/store/             # rusqlite schema + migrations (refinery or hand-rolled), repositories
    src/workspace/         # Workspace, Project, Worktree, Section, ContextGroup
    src/git/               # gix reads, git CLI writes, worktree lifecycle, status/diff cache
    src/forge/             # GitHub/GitLab: PRs, checks, review comments
    src/terminal/          # PTY spawn (alacritty_terminal::tty), Term state per session, OSC 7/133/8
    src/agents/            # Provider trait, registry, ACP client, Codex app-server client, Claude stream-json, PTY provider
    src/sessions/          # AgentSession lifecycle, transcript event log, permission broker, restore
    src/search/            # ripgrep libs, fuzzy (nucleo)
    src/watcher/           # notify + debounce → FsEvent
    src/scripts/           # per-repo setup / run / teardown scripts (ide.toml)
    src/socket/            # JSON-RPC server for CLI/remote
    src/handle.rs          # CoreHandle (in-process)
  ide-theme/               # tokens from design/tokens.json → gpui-component Theme + semantic colors, type scale, spacing, radii, shadows, motion
  ide-ui/                  # GPUI views/elements. No business logic. Talks only to CoreHandle.
    src/shell/             # window chrome, titlebar, sidebar, statusbar, dock host, PiP windows
    src/chat/              # transcript list (virtualized), message blocks, tool-call cards, diff cards, plan, permission prompts, composer (@mentions, /commands, images)
    src/terminal/          # TerminalElement (custom gpui Element over alacritty grid)
    src/diff/              # DiffElement (unified/split, virtualized, intraline), comment gutter
    src/editor/            # file tabs wrapping gpui-component code editor
    src/explorer/          # file tree, worktree-aware
    src/review/            # changes panel, git button state machine, PR panel, checks
    src/palette/           # command palette, quick open, agent picker
    src/settings/          # settings UI
  ide-app/                 # binary: main.rs, app bootstrap, keymap loading, single-instance, deep links
  ide-cli/                 # binary `ide`: talks to the socket
  ide-mock-agent/          # binary: fake ACP agent for tests (scripted responses, permission requests, diffs)
  ide-platform/            # macOS/Linux/Windows specifics: app data dirs, keychain, notifications, dock badge, backlight? (no), single-instance lock
assets/                    # fonts (from design), icons (SVG), sounds
design/                    # the user's design (source of truth for ide-theme and every view)
docs/                      # these documents
```

Dependency direction: `ide-app → ide-ui → ide-theme, ide-proto, ide-core(handle only)`; `ide-cli → ide-proto`; `ide-core → ide-proto`. `ide-ui` never imports `gix`, `alacritty_terminal::tty`, or `tokio` directly (it may use `alacritty_terminal::Term` grid types for rendering, via a read lock the core hands it).

## 3. Domain model (ide-proto)

```
Workspace { id, name, root_dirs[], theme_override?, layout_id, settings_overrides }
Project   { id, workspace_id, repo_root, default_base_branch, worktree_dir, scripts: {setup, run: {name: cmd}, teardown, cleanup}, provider_routing }
Worktree  { id, project_id, path, branch, target_branch, kind: Primary|Task, status: Idle|AgentRunning|NeedsAttention|Conflicted|ReadyToMerge, pr?: PullRef, created_at, archived_at? }
Section   { id, workspace_id, name, rule: Manual(ids) | Filter{ assignee?, branch_glob?, pr_state?, checks?, agent?, run_state? }, order }
ContextGroup { id, name, feature_md_path, worktree_ids[] }           # "shared context" across repos
Tab       { id, worktree_id, kind: Chat(session_id) | Terminal(term_id) | File(path) | Diff(DiffRef) | Browser(url) | PullRequest(PullRef), title }
Layout    { id, tree: Split{axis, ratio, children} | Tabs{tab_ids, active} , pip_windows[] }
AgentSession { id, worktree_id, provider_id, external_session_id?, model?, mode?, status, cwd, created_at, last_activity, cost? }
TranscriptEvent { session_id, seq, ts, kind: UserMessage|AgentChunk|Thought|ToolCall{id,kind,status,title,locations,content}|ToolUpdate|Diff|Plan|Permission{request,resolution}|Mode|Error|Result }
Terminal  { id, worktree_id, pid, cwd (OSC 7), title, shell, cols, rows }
Provider  { id, name, kind: Acp{cmd,args,env} | CodexAppServer | ClaudeStreamJson | Pty{cmd}, installed: bool, version?, auth_state }
```

All ids are `u64` ULIDs stored as TEXT in SQLite. Every event carries a monotonically increasing `seq` per stream so the UI can resume after a hiccup.

## 4. Agent layer

### 4.1 `Provider` trait (ide-core/src/agents/mod.rs)

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;             // name, icon, capabilities (modes, models, load_session, image input, mcp passthrough)
    async fn ensure_installed(&self, progress: Sender<InstallProgress>) -> Result<()>;
    async fn auth_state(&self) -> AuthState;                // Ok | NeedsLogin{instructions} | Unknown
    async fn open_session(&self, req: OpenSession) -> Result<Box<dyn SessionDriver>>; // new or load
}

#[async_trait]
pub trait SessionDriver: Send {
    async fn prompt(&mut self, blocks: Vec<PromptBlock>) -> Result<()>;   // text, @file refs (resource_link), images
    async fn cancel(&mut self) -> Result<()>;
    async fn set_mode(&mut self, mode: ModeId) -> Result<()>;
    async fn set_model(&mut self, model: ModelId) -> Result<()>;
    fn events(&mut self) -> BoxStream<'_, SessionEvent>;                  // normalized TranscriptEvent kinds
    async fn resolve_permission(&mut self, id: PermissionId, outcome: PermissionOutcome) -> Result<()>;
    async fn close(self: Box<Self>) -> Result<()>;
}
```

### 4.2 ACP client (`agents/acp.rs`)
- Spawn `cmd args` with `cwd = worktree.path`, stdio piped, using `agent-client-protocol` `Client.builder()`; implement the client callbacks:
  - `session/request_permission` → route to the permission broker (UI prompt, or auto-resolve by policy: per-worktree mode `Supervised | AcceptEdits | FullAccess`).
  - `fs/read_text_file`, `fs/write_text_file` → served from the worktree with path-jail checks (must be inside the worktree or an allowed extra dir); writes go through the buffer manager so open editor tabs update live.
  - `terminal/*` → backed by our terminal service; the agent's terminals appear as collapsible cards in the chat and can be popped out to a terminal tab.
  - `elicitation/create` → generic form modal.
- `session/update` notifications are mapped 1:1 into `TranscriptEvent` and persisted **before** being forwarded to the UI (crash-safe transcripts).
- `session/load` used for restore when the agent advertises `loadSession`; otherwise we replay our stored transcript read-only and start a fresh session for new prompts, marked "continued".
- Agent discovery: fetch the ACP registry JSON (cached), plus `~/.config/<app>/agents.json` for custom agents; installation via npm/binary distribution info into `<data_dir>/agents/<name>/<version>/`. Bundle a pinned Node runtime? **No** — require Node on PATH for npm-distributed adapters; show a one-click instruction if missing (Phase 4), evaluate bundling in Phase 10.

### 4.3 Codex app-server client (`agents/codex.rs`) — Phase 4b
- `codex app-server` over stdio; map `thread/*`, `turn/*`, `item/*` to `TranscriptEvent`; approvals via `item/commandExecution/requestApproval` and `item/fileChange/requestApproval`; expose `thread/fork`, `review/start`, `account/rateLimits/read` as provider extras in the UI. Fall back to `codex-acp` if the app-server protocol version is unknown.

### 4.4 Claude stream-json (`agents/claude_stream.rs`) — Phase 4c, fallback only
- Bidirectional NDJSON; parse `system/init` capabilities; `assistant`/`user` messages with `parent_tool_use_id` for subagent trees; permission prompts via `--permission-prompt-tool` pointing at a tiny MCP server we host on a socket.

### 4.5 PTY provider (`agents/pty.rs`)
- Any command in a terminal tab, tagged as an "agent terminal" so the sidebar shows activity (bytes/sec, last output line, OSC 133 prompt state → idle/running). This is Snor's Flow Mode and super.engineering's "custom CLI agents".

### 4.6 Permission broker
- Policy per worktree: `Supervised` (ask for everything not read-only), `AcceptEdits` (auto-allow file edits inside the worktree, ask for commands), `FullAccess` (allow all; agent sandboxing is the agent's job). Remembered answers per tool+pattern for the session. Every decision is a `TranscriptEvent::Permission`.
- Notifications (`ide-platform`): OS notification + sidebar badge + dock badge when a session needs attention and the window is not focused.

## 5. Worktree lifecycle (`workspace/` + `git/`)

1. `create_task_worktree(project, name, base?)`: pick branch name (slug from task title or agent-suggested), `git worktree add -b <branch> <worktree_dir>/<slug> <base>`; register in store; emit `WorktreeCreated`; run `scripts.setup` in a terminal tab (streamed), e.g. `pnpm install`, copying `.env` files as declared in `ide.toml`.
2. Status engine: `gix` status + ahead/behind vs target + conflict detection, refreshed on FS events (debounced 150 ms) and after git commands; results cached per worktree; the sidebar shows changed-file counts and status color.
3. Adaptive git action (state machine, `review/git_button.rs` in UI, logic in core):
   `Dirty → Commit`, `Committed & unpushed → Push`, `Pushed & no PR → Create PR`, `PR open & checks failing → Fix CI (dispatch to agent with failing job logs)`, `PR conflicts → Resolve (dispatch)`, `PR mergeable → Merge`, `Merged → Delete worktree`. Each state is computed from status + forge data; each action is one click and opens an agent prompt where an agent is the right tool.
4. Delete: refuse if unpushed commits unless forced; `git worktree remove`, `git branch -d`, run `scripts.cleanup`, archive rows (transcripts are kept).
5. Primary worktree = the original checkout (starred) for quick edits; never auto-deleted.

## 6. Terminal (`core/terminal` + `ui/terminal`)

- Core: `alacritty_terminal::tty::new` for PTY (ConPTY on Windows), `Term<EventProxy>` per terminal guarded by `Arc<FairMutex<Term>>`; reader thread feeds the VTE parser; resize/scroll/selection/search via the alacritty API; OSC 7 (cwd), OSC 133 (prompt marks → command boundaries and "running/idle" state), OSC 8 (hyperlinks), OSC 52 (clipboard, gated by setting), bracketed paste, kitty keyboard protocol optional.
- UI: `TerminalElement` implements `gpui::Element`: `request_layout` → size; `prepaint` → take the read lock, snapshot visible rows into a `Vec<ShapedRow>` cache keyed by (row content hash, width); `paint` → background rects batched by run, glyphs via `window.text_system().shape_line` cached per row, cursor, selection, hyperlink underline on hover. Repaint only on `Wakeup` events from the term, coalesced to the display refresh. Scrollback rendering uses the term's display offset; the UI never copies the whole grid.
- Input: full key → escape sequence mapping (copy Zed's `mappings/keys.rs` semantics), IME, mouse reporting modes, scroll.
- Font: monospace from `design/`; ligatures on; cell metrics computed once per font size.

## 7. Chat transcript (`ui/chat`)

- Virtualized list (gpui-component `VirtualList` or a custom `uniform_list`-style element) over `TranscriptEvent`s grouped into blocks: user message, agent markdown (streaming, incremental parse), thought (collapsed), tool-call card (kind icon: read/edit/execute/search/fetch/think; status; locations → clickable; embedded diff or terminal output), plan (checklist), permission prompt (inline, keyboard-first: `y`/`n`/`a`), result/cost line.
- Streaming markdown: re-parse only the trailing open block on each chunk; code blocks are highlighted with tree-sitter incrementally.
- Composer: multi-line; `@` opens fuzzy file picker (inserts `resource_link` blocks); `/` lists agent-provided commands (`available_commands` from ACP) and our own (`/model`, `/mode`, `/new`, `/worktree`); image paste; queue prompts while the agent is busy (ACP v2 steer if supported, else queue).

## 8. Review (`ui/review`, `ui/diff`)

- Changes panel per worktree: file list (status glyphs), grouped Staged/Unstaged/Untracked, or "turn-by-turn" (diffs attributed to agent turns via `TranscriptEvent::Diff` + timestamps).
- `DiffElement`: unified and split modes; virtualized hunks; intraline highlights (`similar` `TextDiff::from_words` per changed line pair); syntax colors from tree-sitter over the post-image; gutter with line numbers; per-line comment threads (stored in SQLite, anchored by (path, side, line, content hash) and re-anchored after edits); "Send comments to agent" composes one prompt with the file/line context and dispatches to the worktree's active session (this is super.engineering's line-anchored comment feature).
- PR panel: title/body generated by an agent (one-shot prompt to the worktree session or a lightweight provider), checks list with logs (forge API), review comments from the forge mirrored as threads.

## 9. Layout & sessions (`ui/shell`, `core/store`)

- gpui-component `DockArea` for the center with nested splits + tabs; left sidebar (worktrees grouped by Sections; collapsible; filter/search), right panel (review/changes/PR; toggleable), bottom status bar; PiP: secondary GPUI windows (`cx.open_window`) that host a single tab (terminal or chat), always-on-top via `ide-platform`.
- Layout serialization on every change (debounced 500 ms) into `Layout` rows; on start: restore windows, sidebar state, dock tree, active tab, terminals (re-spawn shells at their last cwd; agents' sessions `session/load` when supported), scroll positions. Startup shows the shell within the first frame and hydrates tabs lazily (only the active tab per pane is materialized; others on first focus).

## 10. Orchestration (Phase 8)

- Team = lead session + N specialist sessions, each in its own worktree (or same worktree, sequential). Coordination state lives in `<worktree>/.ide/coordination.json` (agents can read it as a file) and in the store; the CLI exposes `ide agent send <session> "<msg>"`, `ide agent list`, `ide coordination-state get|set`, `ide team run <spec.toml>`. Agents call the CLI from their own tools (it is on PATH inside our terminals with `IDE_SOCKET` set), which is how "agents message one another" works without a new protocol. Context groups (`FEATURE.md`) are prepended to prompts as `resource_link`s.

## 10b. Web access (`insyde-core/src/web/`, `web/`)

- **Server** (`web/mod.rs`, `http.rs`): std `TcpListener`, one thread per connection. `GET` serves the client from `include_bytes!` (CSP: self only); `/ws?t=<token>` upgrades to a WebSocket with a hand-rolled RFC 6455 codec, so the socket's read and write halves live on two threads (a cloned `TcpStream` each). `/auth` lets the client tell a bad pairing from an unreachable host.
- **Pairing**: a 256-bit hex token in `<data dir>/web-token` (0600), compared in constant time, carried in the link fragment (`/#t=…`, never sent in HTTP requests except the WebSocket query) and stored by the browser. "New link" rewrites it and drops every socket.
- **Hub** (`hub.rs`): shared by all clients and kept alive across server restarts. Owns web agent threads (`AcpSession` per store session, lazily started) and browser terminals (`pty.rs`). Requests are JSON `{id, m, p}`; memory-only calls (keystrokes, resize, cancel) run in order on the socket thread, anything touching git or disk on its own thread.
- **Streaming**: an agent's notify marks its thread dirty; one flusher thread wakes at most ~30×/s and sends `{"ev":"thread", len, set:[[i,item]…], meta}` with only the items whose JSON changed (the last 24 while streaming, all of them when a turn ends). Sidebar status changes are broadcast separately.
- **Terminals**: raw PTY bytes (alacritty's `tty`, fd switched back to blocking) go out as binary frames `[1][u32 id][bytes]`; xterm.js emulates. A 512 KB replay buffer is sent on attach under the same lock the pump holds, so a client never misses or doubles output. Terminals outlive the tab.
- **Reach**: `127.0.0.1` by default; all interfaces for LAN/Tailscale; optional `cloudflared tunnel --url` for a public https link. Runs inside the app (Settings › Web access) or headless (`insy serve`).
- A slow client is cut off (bounded queue, socket shutdown) rather than buffered without limit.

## 11. Threading & performance rules

- Main thread: layout, paint, input, entity updates. Never `block_on`, never file I/O, never `Mutex` held across an await.
- Core: `tokio` multi-thread; CPU-heavy work (diff, highlight of big files, search) on `tokio::task::spawn_blocking` or `rayon`.
- UI ↔ core: `CoreHandle::request` returns a future you `cx.spawn` on; events arrive on a `flume` receiver polled by a single GPUI task that dispatches to entities (`cx.update_entity`).
- Caches: shaped-line cache (terminal, diff), highlight cache per (path, buffer version), status cache per worktree, forge cache with ETag.
- Frame watchdog (debug): warn on main-thread tasks > 4 ms; `tracing-tracy` feature for profiling.
- Memory: transcripts are not held fully in memory; the chat list loads a window of events from SQLite (page size 200) and drops far-off pages.

## 12. Security

- Path jail for agent fs calls; symlink resolution; refusing writes outside the worktree unless the user adds an "extra dir".
- OSC 52 clipboard write off by default; OSC 8 links open only via explicit click with confirmation for non-http schemes.
- Forge tokens in the OS keychain (`keyring`), never in SQLite.
- Socket is user-only permissions (0600), a random token in `IDE_SOCKET_TOKEN` env for agents' terminals.
