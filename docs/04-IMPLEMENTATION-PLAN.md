# 04 — Implementation plan (for Opus)

Rules of engagement are in `AGENTS.md`. This file is the ordered work. Each phase ends in a runnable app and a demo checklist; do not start the next phase until the checklist passes on macOS. Each phase is one branch (`phase/N-name`) merged when green.

Estimated sizes are relative (S/M/L/XL), not hours.

---

## Phase 0 — Bootstrap (S)

Deliverables
- Cargo workspace with crates from `03-ARCHITECTURE.md` §2 (empty modules are fine), `rust-toolchain.toml` (stable), `[workspace.dependencies]` pinning every crate from `02-STACK.md`, `deny.toml`, `rustfmt.toml`, `clippy.toml`.
- `gpui = { package = "gpui-pre", version = "=<same as gpui-kit>" }` and `gpui-kit = "0.6"`; verify both compile together (`cargo tree -d` shows a single gpui).
- `ide-app` opens a window with the design's background color, custom titlebar (macOS traffic lights inset per design), and a "Hello" label in the design's font. Fonts loaded from `assets/fonts` via `cx.text_system().add_fonts`.
- `ide-theme`: parse `design/tokens.json` (create it from `design/` if the design has no tokens file; see `AGENTS.md`) → `Theme` struct, hot-reload in debug (`notify`).
- GitHub Actions: build + clippy + test on macOS/Linux/Windows; `cargo nextest`.
- `docs/DEV.md`: how to run, profile (`--features tracy`), take screenshots (`ide-app --screenshot out.png` flag renders one frame and exits; needed for verification).

Checklist: `cargo run --release` opens the window in < 400 ms cold; CI green on 3 platforms.

## Phase 1 — Shell, layout, palette, settings (M)

Deliverables
- Window chrome from the design: titlebar (workspace switcher, search/palette trigger, global actions), left sidebar (resizable, collapsible, `Cmd+B`), right panel (`Cmd+Shift+B`... use the design's keymap if it defines one), status bar.
- Center: gpui-component `DockArea` with tabs, drag-to-split, close, reorder, `Cmd+1..9`, `Cmd+W`, `Cmd+\` split.
- Command palette (`Cmd+K`/`Cmd+Shift+P`): actions registry (`gpui::actions!`), fuzzy via `nucleo`, recent-first.
- Keymap: default `assets/keymap.json` + user override; settings JSON with schema; settings UI (simple form from the design).
- Theme: light/dark/system from tokens; runtime switch without restart.
- Store: SQLite opened from `ide-platform::data_dir()`, migrations; layout persisted and restored (empty tabs are placeholder "Welcome" tabs for now).
- Frame watchdog + `tracing` file logs.

Checklist: layout survives restart; palette runs every action; all views use tokens only (grep for hardcoded `rgb(` in `ide-ui` returns nothing).

## Phase 2 — Workspaces, projects, worktrees (M)

Deliverables
- Open folder/repo → Project; primary worktree starred. Multiple projects per workspace.
- Sidebar: worktrees grouped by project and Sections (manual + filter rules), status color, changed-file counts, branch, PR badge placeholder, running-agent indicator placeholder.
- Create task worktree (dialog: title → branch slug, base branch picker with `gix` refs, "run setup script" toggle); delete (with unpushed-commit guard); rename; archive.
- `ide.toml` in repo root: `[scripts] setup = "pnpm i" ; run.dev = "pnpm dev" ; teardown = ...` + `[worktree] dir = "../.worktrees/{repo}"` + `[env] copy = [".env"]`. Setup runs in a terminal tab (Phase 3 provides it; until then, stream to a log tab).
- Status engine (`gix` status, ahead/behind vs target, conflicts) on FS events; `git` CLI wrapper with structured errors.
- Tests: worktree create/delete on a temp repo; status changes on file writes.

Checklist: create 5 worktrees in a real repo in < 2 s wall; sidebar updates live when files change from outside.

## Phase 3 — Terminal (L)

Deliverables
- `core/terminal`: spawn shell in worktree cwd with `alacritty_terminal`, env (`IDE_SOCKET`, `IDE_WORKTREE`, `TERM=xterm-256color`, `COLORTERM=truecolor`), resize, close, OSC 7/133/8/52, scrollback 10k lines (setting), search.
- `ui/terminal::TerminalElement`: rendering per `03-ARCHITECTURE.md` §6; selection (click-drag, double/triple click, block select), copy/paste, hyperlinks, cursor styles, blink (stops when unfocused; zero idle CPU), font size zoom, scrollbar, "jump to previous/next prompt" using OSC 133 marks, find in terminal.
- Terminal tabs and splits inside the dock; "open terminal here" from explorer/worktree context; Flow Mode (`Cmd+Shift+F`: hide sidebars, grid of all terminals of the worktree).
- Sidebar shows terminal activity per worktree (running command, idle, needs input via OSC 133 + bytes/sec heuristics).
- Restore: shells re-spawned at last cwd on startup.

Checklist: `vim`, `htop`, `claude` (interactive TUI), `codex` render correctly; `cat` a 100 MB file does not stall the UI; a terminal with no output uses 0% CPU.

## Phase 4 — Agents over ACP (XL)

4a. Core
- `Provider` + `SessionDriver` traits; registry from ACP registry JSON + `agents.json`; install flow (npm into app dir, or binary download with checksum) with progress UI; auth state detection (run `initialize`, inspect `authMethods`, show provider-specific login instructions and a "Login in terminal" button that opens a terminal running the agent's login command).
- ACP client with all client-side methods (permission, fs, terminal, elicitation); path jail; transcript persistence before forwarding; cancel; `session/load`.
- `ide-mock-agent` binary: scripted ACP agent (text chunks, thought, tool call with diff, terminal tool, permission request, plan, error) used in integration tests.

4b. UI
- Chat tab per session: transcript list per `03-ARCHITECTURE.md` §7; composer with `@file` and `/commands`; model/mode pickers from `initialize`/`session/new` capabilities; permission prompts inline + keyboard; tool-call cards with embedded diffs (reuse Phase 6 `DiffElement` in read-only mode, or a simple hunk renderer now and swap later) and terminal output cards; plan checklist; cost/usage line when the agent reports it.
- Multiple sessions per worktree; new chat, rename, archive; sidebar shows session state (Idle / Running / Needs permission / Error) with badges; OS notifications when unfocused.
- Provider order: Claude Code (`@agentclientprotocol/claude-agent-acp`) → Gemini CLI (`--acp`) → OpenCode → Codex via `codex-acp`. Each must pass the mock-agent test suite plus one live smoke test documented in `docs/DEV.md`.
- PTY provider: "Run any CLI as agent" opens a terminal tab tagged as agent.

4c. Extras
- Codex app-server client with thread fork / review / rate limits (skip if time-boxed; ACP path is complete without it).
- Claude stream-json fallback provider (only if the ACP adapter is unavailable on the machine).

Checklist: start Claude Code in a worktree, ask it to edit a file, approve the edit inline, see the diff card, open the file tab showing the change, cancel a running turn, quit the app, relaunch, transcript is intact and the session resumes with `session/load`. Same for Gemini and OpenCode. Three sessions streaming concurrently keep 120 fps.

## Phase 5 — Files and editor (M)

Deliverables
- Explorer (gpui-component Tree, virtualized): worktree-rooted, follows the focused terminal's cwd (Snor's idea, as a toggle), git status decorations, create/rename/delete/reveal, drag to composer to mention.
- File tabs using gpui-component code editor: tree-sitter highlight for the grammar list in `02-STACK.md`, line numbers, minimap off by default, find/replace, go-to-line, save (`Cmd+S`), external-change reload, dirty indicator, large-file guard (> 5 MB opens read-only plain).
- Quick open (`Cmd+P`) with `nucleo` over `ignore`-walked files; project search (`Cmd+Shift+F`) with `grep-searcher`, results tab, click → editor at line.
- Agent `fs/write_text_file` updates open buffers live; edits made by the user while the agent runs are not clobbered (write-through with version check → the agent gets an error and re-reads).
- Optional (feature flag): LSP for TypeScript and Rust via gpui-component's LSP support (`typescript-language-server`, `rust-analyzer` if on PATH).

Checklist: open a 50k-line file and scroll at 120 fps; agent edits appear live; quick open over a 100k-file repo answers in < 50 ms per keystroke.

## Phase 6 — Review and git workflow (L)

Deliverables
- Changes panel (right panel): per-worktree file list; staged/unstaged/untracked; turn-by-turn grouping from transcript diffs; stage/unstage/discard (with confirm); open in DiffElement.
- `DiffElement`: unified + split, virtualized, intraline, syntax-colored, collapsible unchanged regions, whitespace toggle, `]c`/`[c` hunk navigation; also used for tool-call diff cards and for PR file views.
- Line comments: add/edit/resolve threads on any diff line; "Send to agent" builds one prompt with quoted context and dispatches to the worktree's active session (or asks which). Threads persist and re-anchor.
- Adaptive git button per `03-ARCHITECTURE.md` §5.3; commit dialog with agent-generated message (one-shot prompt to a lightweight provider session; editable); push; create PR/MR (GitHub REST via `octocrab`, GitLab REST), draft toggle, base branch, template from `.github/PULL_REQUEST_TEMPLATE.md`; checks list with live status polling and "Fix CI" that fetches the failing job log and dispatches it; conflicts detection and "Resolve with agent"; merge (method setting) and delete worktree.
- Forge auth: reuse `gh auth token`/`glab auth token`; else device flow; store in keychain.
- Sidebar Sections filters can now use PR state and checks.

Checklist: from a dirty worktree, one click each: commit → push → PR → (simulate failing check) fix → merge → delete. Comment on a diff line and watch the agent address it.

## Phase 7 — CLI, socket API, notifications, PiP, browser (M)

Deliverables
- `ide-core/socket`: JSON-RPC 2.0 over unix socket / named pipe; token auth; method set mirrors `ide-proto` requests + event subscription.
- `ide` CLI: `ide open <path>`, `ide worktree list|create|delete|status`, `ide agent list|send|prompt|cancel`, `ide session list|show`, `ide tab open (chat|terminal|file|diff|browser)`, `ide layout save|load`, `ide review comments`, `ide instructions <topic>` (prints the relevant docs section so agents can learn the CLI, as super.engineering does).
- Terminals get `IDE_SOCKET`/`IDE_SOCKET_TOKEN`/`IDE_WORKTREE` so agents can call `ide` from inside their tools.
- Notifications center in the status bar; OS notifications with actions (Approve / Open); Do-not-disturb.
- PiP: pop any tab into a floating always-on-top window; layout persistence includes PiP windows.
- Browser tab (gpui-component webview): URL bar, reload, open the dev server URL from `ide.toml` `run.dev` when it prints a URL (detect via terminal output regex), devtools toggle. Mark as optional feature `browser`.

Checklist: `ide agent send <session> "run the tests"` from a terminal inside the app streams into the chat tab; PiP terminal stays on top; browser tab shows the running dev server.

## Phase 8 — Orchestration: teams and shared context (L)

Deliverables
- Team spec (`.ide/team.toml` or CLI flags): lead provider/model, specialists with roles and their own worktrees (auto-created from the same base), prompts/roles, hand-off rules. `ide team run <spec>` and a "Run team" UI.
- Coordination state file + store; `ide coordination-state get|set|watch`; agents message each other with `ide agent send`; the lead's chat shows a team timeline (who did what, hand-offs, blocked-on).
- Context groups across repos: create a feature spanning N projects → `FEATURE.md` + shared instructions; materialize child worktrees on demand; unified review across the group's worktrees.
- Sidebar: team view (roles, status), group view.

Checklist: run a 3-agent team (lead + 2 specialists) on a sample task; the lead delegates via CLI, specialists report back, all visible; unified diff across two repos.

## Phase 9 — Remote hosts over SSH (L, optional)

- `ide-server` headless binary (core without UI, socket over stdin/stdout); local app connects via system `ssh` (ControlMaster), uploads/updates the server binary per platform, and mounts a remote workspace: worktrees, terminals, agents, git all run remotely; files streamed to the local editor; diff computed remotely. Mirrors Zed's remote architecture.

## Phase 10 — Cross-platform, packaging, hardening (M)

- Windows (ConPTY path already in alacritty_terminal; named pipes; DirectX) and Linux (Vulkan; `xdg` dirs; desktop file) parity passes; platform quirks isolated in `ide-platform`.
- Packaging: `cargo-packager` dmg/msi/AppImage; codesign + notarize; auto-update; crash reporting opt-in (`sentry` or minidump upload), anonymous telemetry opt-in (off by default).
- Perf pass against `02-STACK.md` budgets with `tracy`; memory pass (transcript paging, shaped-line cache bounds, grammar unloading).
- Accessibility pass (AccessKit roles from gpui-component; VoiceOver smoke test), keyboard-only navigation of everything.
- Docs: user docs for concepts (workspace/project/worktree/tab/section/group), keybindings, `ide.toml`, CLI, providers.

---

## Verification standard for every phase

1. `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo nextest run` green locally and in CI.
2. `ide-app --screenshot` images of every new view attached to the PR and compared against `design/` side by side; pixel-level intent (spacing, type scale, colors) must match tokens.
3. Frame watchdog reports zero main-thread violations during the demo checklist.
4. The demo checklist above executed manually on macOS and written up in the PR description with what was observed.
