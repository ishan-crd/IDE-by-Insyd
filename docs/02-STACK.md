# 02 — The stack (decided)

One sentence: **a native, GPU-rendered, 100% Rust desktop IDE built on GPUI (Zed's UI framework) with gpui-kit components, driving coding agents over the Agent Client Protocol, with an alacritty-based terminal, gitoxide for git, and an embedded daemon core that a CLI and (later) remote clients can talk to.**

Rename note: the workspace is called `ide` and crates are `insyde-*` until a product name is chosen. Renaming is a single `sed`; do not block on it.

## Non-negotiables

1. **No Electron, no WebView for the app chrome.** WebView (wry) is allowed for exactly one thing: the optional Browser tab. The separate *web client* (a browser talking to the app over the network) is HTML/JS by necessity; it has no logic of its own beyond rendering.
2. **Stable Rust toolchain**, pinned in `rust-toolchain.toml` (Zed and gpui-kit build on stable). If a dependency demands nightly, replace the dependency.
3. **Every long operation off the main thread.** Main thread does layout + paint only. Budget: no main-thread task over 4 ms in debug builds (a frame watchdog logs offenders).
4. **Agents talk protocols, not scraped terminals.** ACP first, Codex app-server second, Claude `stream-json` as a fallback. A raw PTY provider exists for arbitrary CLIs, but it is a terminal tab, not a chat.
5. **Local-first, credential-free.** We never proxy keys, code, or chat. Agents run as subprocesses with the user's own login.
6. **Design is law.** The UI is implemented from `design/` (see `AGENTS.md`), through design tokens in `ide-theme`. No ad-hoc colors, sizes, or fonts in views.

## Layer by layer

| Layer | Choice | Version pin | Alternatives considered |
|---|---|---|---|
| Language | Rust 2024 edition, stable | `rust-toolchain.toml` = `channel = "1.98.1"` (what Zed pins today; gpui-kit builds on stable too) | — |
| UI framework | **GPUI** via `gpui = { package = "gpui-pre", version = "=X" }` | Must equal the version `gpui-kit` pins (0.3.6 as of 2026-09-21). Bump both together, weekly at most. | Tauri/Dioxus (WebView), egui (immediate mode, Snor), iced, Xilem (alpha). See `01-RESEARCH.md` §2. |
| Components | **gpui-kit 0.6.x** (`gpui-component` + `gpui-base`): Dock (nested splits + draggable tabs), Resizable, Tabs, Tree, VirtualList, Table, Input/code editor (tree-sitter + LSP), Markdown, Modal/Popover/Menu/Tooltip, Notification, AccessKit. | 0.6.6 | Writing every widget by hand (what Zed does). We use gpui-kit for chrome and the editor; we write our own Terminal, Diff, and Chat-transcript elements because they are performance-critical and design-specific. |
| Theme | `ide-theme` crate generating gpui-component's `Theme` + our own semantic tokens from `design/tokens.json`. | — | — |
| Text editor | gpui-component `Input` in code-editor mode (rope-backed, tree-sitter highlight, LSP diagnostics/completion/hover, 200k lines). | 0.6.x | Own editor on `ropey` + `tree-sitter` (Phase 10 option if the component blocks the design). |
| Terminal | **`alacritty_terminal` 0.26** for PTY, VTE parsing, grid, scrollback, selection, search + our own GPUI `TerminalElement` (modelled on Zed's `terminal_view`). | 0.26.0 | `gpui-terminal` (6 commits, no selection/scrollback), `vt100` (unmaintained, what Snor uses), `wezterm-term` (heavy). |
| Agent protocol | **`agent-client-protocol` 2.2** (official Rust SDK). We implement the ACP *Client* side. | 2.2.0 | Per-agent bespoke integrations. |
| Agent adapters | Claude Code: `@agentclientprotocol/claude-agent-acp` (npm, auto-installed to app data dir via bundled Node or `npx`). Codex: `codex-acp` for parity, then `codex app-server` for extra features (thread fork, review/start, rate limits). Gemini CLI: `gemini --acp`. OpenCode, Copilot, Cursor, Kimi, Qwen, Goose, Factory, Kiro: native ACP per registry. | registry JSON | — |
| Claude raw path | `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --replay-user-messages` | Claude Code ≥ 2.1.259 | Used only if the adapter is unavailable. |
| Git (read) | **`gix` 0.87** (gitoxide): status, diff, log, refs, worktrees, blame, index. | 0.87.1 | `git2` (C dep, libgit2). |
| Git (write) | System `git` binary via `tokio::process` (`worktree add/remove`, `commit`, `push`, `fetch`, `rebase`, `merge`, hooks respected). | git ≥ 2.40 | gix push/merge are not finished. |
| Diff engine | `similar` 3.2 (line + word-level intraline diff, unified/split rendering) | 3.2.0 | `imara-diff` (used internally by gix; fine too). |
| Forge APIs | `reqwest` (rustls) + `octocrab` for GitHub; GitLab via REST. Auth: `gh auth token` / `glab auth token` if present, else device-flow stored in `keyring`. | — | Shelling out to `gh` for everything (works but slow and brittle). |
| Async runtime | GPUI's executors for UI-adjacent futures; **one `tokio` multi-thread runtime** in `ide-core` for subprocesses, sockets, HTTP, git. Bridge: `async-channel` / `tokio::sync::mpsc` + `cx.spawn` on the UI side. | tokio 1.53 | — |
| Persistence | **SQLite via `rusqlite` (bundled)** in the app data dir: workspaces, projects, worktrees, sections, sessions, transcripts (JSON blobs per event), layouts, recent files, settings overrides. | 0.40 | JSON files (fine for settings, not for transcripts). |
| Settings / keymap | JSON with comments (`json5`) at `~/.config/<app>/settings.json` and `keymap.json`, plus per-workspace `.<app>/settings.json`. Schema via `schemars` for editor completion. | — | TOML (worse for nested UI config). |
| FS watching | `notify` 8 + `notify-debouncer-full` | 8.2 | — |
| Search / ignore | `ignore` + `grep-searcher` + `grep-regex` (ripgrep libs); fuzzy matching `nucleo` | — | Shelling out to `rg` (fallback if not vendored). |
| Syntax | `tree-sitter` 0.27 + grammar crates (rust, typescript, tsx, javascript, python, go, json, toml, yaml, markdown, bash, css, html, c, cpp, java, ruby, swift, kotlin) | 0.27 | — |
| Markdown (chat) | gpui-component Markdown renderer for prose; custom code-block element with tree-sitter highlighting and copy/apply actions. | — | — |
| Browser tab | gpui-component `webview` feature (wry). Overlay limitation accepted. | — | CEF off-screen (heavy). |
| IPC (CLI ↔ app) | Unix domain socket (named pipe on Windows) with JSON-RPC 2.0 (`serde_json`), same message types as the in-process core API. | — | HTTP (Arbor). Socket is lower-latency and needs no port. |
| Web client (remote access from a browser) | Plain HTML/CSS/ES modules in `web/`, no build step, compiled into the binary; `xterm.js` 6 (MIT, vendored) for terminals. Served by `insyde-core::web`: a small HTTP + WebSocket (RFC 6455) server on std sockets, thread per connection, no tokio. | xterm 6.0.0 | A Rust/WASM UI (Leptos, Dioxus): adds a wasm toolchain and a second UI stack for no user-visible gain. The browser is only a window onto the Rust core; every rule (git, agents, terminals) still runs in Rust. |
| Remote (Phase 9) | Headless `ide-server` binary over SSH (system `ssh` with ControlMaster), same JSON-RPC core API, like Zed's `remote_server`. | — | `russh` in-process client. |
| Logging | `tracing` + `tracing-appender` (rolling files) + `tracing-tracy` behind a feature for profiling. | — | — |
| Errors | `thiserror` in libraries, `anyhow` in binaries. | — | — |
| Tests | `cargo test`; `gpui::TestAppContext` for view tests; `insta` snapshots for layouts and protocol transcripts; a `ide-mock-agent` binary speaking ACP for end-to-end tests; `cargo nextest` in CI. | — | — |
| Lint | `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo deny` (licenses/advisories). | — | — |
| Packaging | `cargo-packager` (dmg/msi/AppImage), macOS notarization, `velopack` or cargo-packager updater for auto-update. | — | — |
| CI | GitHub Actions matrix: macos-14 (arm64), ubuntu-24.04, windows-2022. Build + test + clippy on every PR; nightly release builds. | — | — |
| Platform order | **macOS first** (Metal), Linux second (Vulkan), Windows third (DirectX). Nothing platform-specific outside `ide-platform`. | — | — |

## Performance budgets (measured in CI on the macOS runner; fail the build if exceeded by 25%)

| Metric | Budget |
|---|---|
| Cold start to first frame (release, warm disk) | < 400 ms |
| Idle RSS, one workspace, one worktree, one terminal, one chat | < 150 MB |
| Idle CPU (no blinking cursor visible) | 0% (no timers when idle; use event-driven repaint) |
| Terminal throughput (`cat` a 100 MB file) | ≥ 200 MB/s parse, frame-rate capped rendering, no UI stall |
| Agent stream token → paint | < 16 ms |
| Diff view of a 10k-line file | first paint < 50 ms, 120 fps scroll |
| Worktree create (git worktree add + setup script kickoff) | UI shows the worktree in < 100 ms; git work is background |
| Session restore (10 worktrees, 30 tabs) | < 300 ms to interactive |

## Things we explicitly do not build

- Our own LLM calls. The agents own their model access. (A future "quick commit message" feature may call an agent in one-shot mode, never an API key we hold.)
- A plugin system, a marketplace, an account system, or telemetry (opt-in anonymous crash reports only, Phase 10).
- A general-purpose editor to rival Zed. The editor exists so you can read and touch files while agents work; the IDE's value is worktrees + agents + review.

## Prerequisites on the dev machine

- `rustup` (not installed on this Mac at the time of writing: `curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh`), then `rustup toolchain install 1.98.1`.
- Xcode command line tools (Metal shaders compile at build time).
- `git` >= 2.40, `node` >= 20 (for npm-distributed ACP adapters), `cargo-nextest`, `cargo-deny`.
- The agent CLIs you want to test with, already logged in: `claude`, `codex`, `gemini`, `opencode`.
