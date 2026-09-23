# 01 — Research: what we are competing with, and what the ecosystem offers (Sept 2026)

This file is the evidence behind the stack choice in `02-STACK.md`. Read it once; you do not need it while coding.

## 1. Products in the category ("agent control plane" / agentic IDE)

| Product | Stack | Model | What is good | What is weak |
|---|---|---|---|---|
| **super.engineering** (closed, alpha, macOS-only) | 100% Rust, GPU-rendered via Metal, no Electron. WebKit only for browser/HTML tabs. Sub-500 ms cold start, ~160 MB idle. | Workspace → Project → Worktree → Tab (chat / terminal / file / diff / browser). Sections (sidebar groups). Shared-context branch groups across repos (`FEATURE.md`). 15 CLI providers as native subprocesses (Claude Code, Codex, Cursor, Gemini, Copilot, OpenCode, Kimi, Qwen, Antigravity, Factory, Kiro, Pi, Hermes...). Teams (`sc team run`, `sc agent send`, coordination state). Session restore (tabs, splits, PiP, chat). Adaptive Git button (commit → push → PR → fix CI → merge → delete). Line-anchored review comments dispatched to agents. SSH remote hosts. `sc` CLI. | Fastest thing in the category; the reference for "feel". | Closed source, macOS-only, no scheduling, experimental teams. |
| **T3 Code** (open, MIT, 23k stars) | Electron + TypeScript monorepo, local daemon (`t3` CLI, installable as a service), web + iOS/Android clients through a relay. Claude via Agent SDK, Codex via app-server. | Repos → threads per project → chat. Worktree per thread, one-click PR, diff viewer (unified/split), integrated terminal, custom actions, supervised / full-access modes, mid-thread model switching. | Best "product" surface; phone access; huge community. | Electron; the UI is a web page. Very early. |
| **Superset** (source-available ELv2) | Web-based desktop app. | 100+ agents, in-app browser, automations, Slack bot, TS SDK, MCP server, remote relay, iPhone app. | Broadest feature set. | Web stack, $20/mo Pro. |
| **Conductor** (free, macOS) | Native macOS (Swift). | Worktree per agent, good review panel. | macOS only. |
| **Arbor** (open, MIT, GPUI) | Rust + GPUI. Daemon (`arbor-httpd`) + GUI + CLI + MCP server + web UI. ACP agents (Claude, Codex, Pi, Gemini). Optional Ghostty VT engine. SSH/mosh "outposts". Requires nightly Rust. | Closest open-source architecture to what we are building. | Rough UI; nightly toolchain. |
| **Waku, OxiMux, Farcaster** (GPUI) | Rust + GPUI, embedded terminals, multi-agent. | Proof that GPUI is the de-facto choice for this category. | Small. |
| **Snor** (user-supplied repo) | Rust, `eframe 0.36`/egui (glow), `ropey`, `tree-sitter-highlight`, `portable-pty` + `vt100`, `notify`. 8.3k LOC, Windows-only (ConPTY + `powershell.exe`, WMI backlight). | Honest about memory (~150 MB). Flow Mode (4 terminal panes). Nice small ideas: explorer re-roots to the focused shell's cwd; "open terminal here". | Immediate-mode egui makes a polished design nearly impossible (no real layout system, no animation model, utilitarian text rendering, cursor bugs documented in its own AGENTS.md). `vt100` crate is unmaintained. No git, no agent protocol, no LSP, no sessions. **Verdict: take ideas, take nothing else.** |

Takeaways that shape our product:
1. Worktree-per-task is the consensus primitive. Everyone does it; the differentiator is the *review* and *orchestration* layer on top.
2. The winners in "feel" are all native GPU-rendered Rust (super.engineering, Arbor, Waku). Electron is what we are beating.
3. Every agent that matters now speaks a JSON-RPC protocol over stdio: **ACP** (Claude via `@agentclientprotocol/claude-agent-acp`, Codex via `codex-acp`, Gemini CLI native, OpenCode/Copilot/Cursor/Kimi/Qwen/Goose/Cline/Factory/Kiro native — 60+ agents, with an install registry). Codex also exposes its own richer `codex app-server`. Claude Code also exposes `--input-format stream-json --output-format stream-json`.
4. A daemon-backed architecture (T3, Arbor, super.engineering's `sc`) is what makes sessions survive restarts, enables a CLI, and later enables remote/phone clients. Design it in from day one.

## 2. Rust GUI frameworks — evaluated for a code editor + terminal IDE

| Framework | Verdict | Why |
|---|---|---|
| **GPUI** (`gpui-pre` weekly snapshots of Zed's crate, 0.3.6 on 2026-09-21; Zed 1.15 stable on macOS/Linux/Windows) | **Chosen** | Built for exactly this: Zed renders a full IDE with it at 120 fps. Metal / DirectX 12 / Vulkan (blade) backends. Hybrid retained/immediate model with `Entity<T>` state, Tailwind-style flex layout (`div().flex().gap_2()`), first-class text shaping, GPU-cached glyph atlas, animations, and a mature focus/keybinding/action system. Ecosystem: **gpui-kit / gpui-component** (0.6.6, 14.7k stars, used in production by Longbridge Pro) provides Dock layout with nested splits + draggable tabs, virtual lists, tables, a 200k-line code editor with tree-sitter + LSP, Markdown/HTML renderers, WebView (wry), AccessKit accessibility. super.engineering, Arbor, Waku all use it. |
| Tauri 2 / Dioxus desktop | Rejected | Both are a system WebView. You get Electron's rendering model (DOM, JS, WebKit/WebView2 quirks) with a Rust backend. Text editor + terminal in a WebView means Monaco/xterm.js in JS: the exact thing the user asked to avoid. |
| egui / eframe (what Snor uses) | Rejected | Immediate mode; utilitarian styling; layout is manual; no retained widget identity for animation/focus; text rendering is CPU-shaped and looks "game UI". Snor's own docs list the cursor/selection bugs. |
| iced | Rejected | Good Elm-style framework (COSMIC desktop) but no IDE-grade text/editor/terminal ecosystem, and its text rendering (cosmic-text) is fine but not GPU-atlas-fast for large editors. |
| Xilem / Masonry / Vello / Parley | Rejected (watch) | Linebender's stack is excellent tech but Xilem is self-described alpha; no dock, no editor, no terminal. |
| Slint, Floem, Makepad | Rejected | Slint is a DSL aimed at embedded/commercial; Floem and Makepad have no IDE ecosystem. |

## 3. Key crates, verified versions (crates.io, 2026-09-23)

| Concern | Crate | Version | Notes |
|---|---|---|---|
| UI | `gpui-pre` (as `gpui`) | =0.3.6 | Pin exact; snapshot weekly. |
| UI components | `gpui-kit` / `gpui-component` / `gpui-base` | 0.6.6 | Pins its own `gpui-pre`; our pin must match theirs. |
| Agent protocol | `agent-client-protocol` | 2.2.0 (2026-09-18) | Official Rust SDK from agentclientprotocol/rust-sdk. `Client.builder()`, v1 stable + v2 draft. 4.5M downloads. |
| Claude Code adapter | `@agentclientprotocol/claude-agent-acp` (npm) | current (0.23.x lineage) | Uses Claude Agent SDK 0.2.83. `@zed-industries/claude-agent-acp` is deprecated. |
| Codex | `codex app-server` (JSON-RPC over stdio/ws/unix) or `codex-acp` | ships with Codex CLI | Thread/turn/item model; approvals via `item/*/requestApproval`; `turn/diff/updated`, `turn/plan/updated`. |
| Terminal emulation | `alacritty_terminal` | 0.26.0 (2026-04) | Same engine Zed's terminal crate uses. Owns PTY + VTE + grid + selection + scrollback + search. |
| Text buffer | `ropey` | 1.6.1 (2.0 in beta; gpui-kit uses 2.0.0-beta.1) | Use whatever gpui-component's editor exposes. |
| Syntax | `tree-sitter`, `tree-sitter-highlight` | 0.27.0 | Grammars as crates: rust, typescript, tsx, javascript, python, go, json, toml, yaml, markdown, bash, css, html. |
| LSP | `lsp-types` 0.97, `async-lsp` 0.2.4 | | Only needed in Phase 6 (optional). gpui-component's editor already speaks LSP diagnostics/completion/hover. |
| Git | `gix` (gitoxide) | 0.87.1 (2026-08) | Pure Rust: status, diff, worktree, refs, index, commit-graph, blame. Push/merge/rebase: shell out to `git` binary. |
| Git fallback | `git2` | 0.21.0 | Not used; libgit2 C dep. Shell out instead. |
| Diff | `similar` 3.2 / `imara-diff` 0.2 | | `imara-diff` is what gix uses; `similar` for unified/split rendering with word-level intraline. |
| FS watch | `notify` 8.2 + `notify-debouncer-full` | | |
| Ignore rules / search | `ignore` 0.4.33, `grep-searcher`, `grep-regex` (ripgrep libs) | | Project-wide search. |
| Async | `tokio` 1.53 | | GPUI has its own executor; bridge with a dedicated tokio runtime thread + channels. |
| Persistence | `rusqlite` 0.40 (bundled) | | Sessions, chat transcripts, layouts, worktree registry. |
| Secrets | `keyring` 4.2 | | Forge tokens (GitHub/GitLab). |
| SSH (later) | `russh` 0.63 | | Or shell out to system `ssh` with ControlMaster like Zed. |
| Serialization / IPC | `serde`, `serde_json`, `schemars` | | Daemon ↔ app ↔ CLI protocol. |
| HTTP | `reqwest` (rustls) | | GitHub/GitLab APIs. |
| Logging | `tracing`, `tracing-subscriber`, `tracing-appender` | | |
| Errors | `anyhow` (bins), `thiserror` (libs) | | |
| WebView (browser tab) | `gpui-component` `webview` feature (wry) | | Overlay-only limitation (cannot draw over it); acceptable for a browser tab. |

## 4. Protocol facts we rely on

**ACP (Agent Client Protocol)** — JSON-RPC 2.0 over stdio.
- Agent methods: `initialize`, `authenticate`, `session/new`, `session/load` (optional), `session/prompt`, `session/set_mode`, `session/cancel` (notification).
- Client methods the IDE must implement: `session/request_permission` (baseline), `fs/read_text_file`, `fs/write_text_file`, `terminal/create|output|release|wait_for_exit|kill`, `elicitation/create`.
- Notifications from the agent: `session/update` carrying agent message chunks, thought chunks, tool calls (with `kind`, `status`, `locations`, `content` incl. diffs), plan entries, available commands, mode changes.
- Paths are absolute; line numbers are 1-based. Custom methods prefix `_`. `_meta` for extensions.
- Registry: `agentclientprotocol/registry` JSON with distribution info (npm/binary) so the IDE can auto-install agents.

**Codex app-server** — JSON-RPC 2.0 (JSONL over stdio; ws and unix socket also available). Methods: `thread/start|resume|fork|list|read|archive`, `turn/start|steer|interrupt`, `review/start`, `model/list`, `account/*`, `config/*`, `skills/list`, `fs/*`, `command/exec`. Notifications: `thread/*`, `turn/started|completed|diff/updated|plan/updated`, `item/started|completed`, `item/agentMessage/delta`, `item/commandExecution/outputDelta`, `item/reasoning/*`, approvals `item/commandExecution/requestApproval`, `item/fileChange/requestApproval`. Schemas via `codex app-server generate-json-schema`.

**Claude Code headless** — `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --replay-user-messages [--resume <id>] [--permission-mode ...] [--allowedTools ...] [--bare]`. Emits `system/init` (with `capabilities` array, `mcp_servers`, `plugins`), `assistant`, `user`, `stream_event`, `system/api_retry`, `system/permission_denied`, `result` (with `total_cost_usd`, `session_id`). Subagent messages carry `parent_tool_use_id`. Permission prompts go through `--permission-prompt-tool` (MCP) or the Agent SDK `canUseTool`. We use ACP as the primary path and keep this as the fallback/raw path.

## 5. Sources
- https://super.engineering/ and /docs/ ; https://superset.sh/compare/superset-vs-super-engineering
- https://t3.codes/ ; https://github.com/pingdotgg/t3code ; https://betterstack.com/community/guides/ai/t3-code/
- https://github.com/MR-STARK87/Snor (cloned and read: Cargo.toml, README.md, AGENTS.md, src/*)
- https://github.com/zed-industries/awesome-gpui ; https://github.com/longbridge/gpui-kit ; https://crates.io/crates/gpui-pre
- https://agentclientprotocol.com/protocol/overview ; https://agentclientprotocol.com/get-started/agents ; https://github.com/agentclientprotocol/rust-sdk ; https://github.com/agentclientprotocol/claude-agent-acp ; https://github.com/agentclientprotocol/registry
- https://learn.chatgpt.com/docs/app-server.md (Codex app-server)
- https://code.claude.com/docs/en/headless
- https://github.com/penso/arbor ; https://zed.dev/docs/remote-development
- https://wrenlearnsrust.com/posts/2026-03-11-rust-gui-landscape-2026.html ; https://linebender.org/blog/tmil-24/
