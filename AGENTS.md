# CLAUDE.md — instructions for the implementing agent (Opus)

You are building InsyDE, a native, GPU-rendered AI-agent IDE in Rust. The plan is in `docs/`; what is already built is in `docs/STATUS.md` (read it first). Crates are `insyde-theme`, `insyde-core`, `insyde-app` (the plan's `ide-*` names). Read in this order before writing code:

1. `docs/02-STACK.md` — the stack. It is decided. Do not re-litigate it or swap crates without updating the doc and saying why.
2. `docs/03-ARCHITECTURE.md` — crates, data model, protocols, threading rules.
3. `docs/04-IMPLEMENTATION-PLAN.md` — phases with checklists. Work strictly in phase order.
4. `design/` — the product design. **It is the source of truth for every pixel.** If `design/` is empty or missing a screen you need, stop and ask the user for it; do not invent a design.
5. `docs/01-RESEARCH.md` — background only.

## Hard rules

- 100% Rust, GPUI (`gpui-pre` snapshot pinned to the exact version `gpui-kit` uses) + `gpui-kit`. No Electron, no Tauri, no WebView except the optional Browser tab.
- Stable toolchain. Never add a nightly-only dependency.
- Main thread does layout/paint/input only. All I/O, git, subprocess, diff, search work happens in `ide-core` on tokio or blocking pools. If you feel the need to `block_on` in a view, the design of that view is wrong.
- Every color, font, size, radius, shadow, and duration in `ide-ui` comes from `ide-theme` tokens. Grep-enforced in CI: no `rgb(`, `px(` literals for spacing, or font names in `ide-ui` (a small allowlist file exists for genuine one-offs).
- Agents are integrated over protocols (ACP first). Never parse a TUI's screen to extract chat.
- Transcript events are persisted before they are shown.
- Keep `docs/` current: when a decision changes, edit the doc in the same PR.

## Design handoff conventions (`design/`)

Expected contents (the user exports these from their design tool):
- `design/tokens.json` — colors (light/dark), type scale, spacing, radii, shadows, motion. If absent, derive it from the design files, write it, and list every value you inferred in the PR.
- `design/screens/*.png|.svg|.html` — each screen/state.
- `design/components/*` — buttons, tabs, cards, inputs, chat blocks, diff rows, terminal chrome.
- `design/README.md` — the user's notes, keymap, and naming.

`ide-theme` is generated from `tokens.json`; views reference tokens by semantic name (`theme.surface.raised`, `theme.text.muted`, `theme.accent.primary`, `theme.status.running`, `theme.diff.added_bg`...).

## Workflow

- One branch per phase (`phase/N-name`). Small commits. PR description contains the phase checklist with observed results and screenshots from `ide-app --screenshot`.
- Before claiming a phase is done: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo nextest run`, then run the app and execute the checklist by hand.
- When a gpui-kit component cannot match the design, first try theming/composition; if it is structurally impossible, write a custom `Element` in `ide-ui` and note it in `docs/03-ARCHITECTURE.md`.
- When an agent adapter misbehaves, write a failing case into `ide-mock-agent` first, then fix.
- Do not add features outside the current phase. Park ideas in `docs/BACKLOG.md`.

## Useful references while coding

- GPUI: read Zed's source for patterns (`crates/gpui/examples`, `crates/terminal_view`, `crates/workspace`). Zed is GPL; **do not copy code**, learn the approach.
- gpui-kit: `https://github.com/longbridge/gpui-kit` examples (`dock`, `editor`, `markdown`, `table`, `virtual_list`).
- ACP: `https://agentclientprotocol.com/protocol/overview` and `https://github.com/agentclientprotocol/rust-sdk` examples (`yolo_one_shot_client.rs`, `v2_one_shot_client.rs`).
- Codex app-server: `https://learn.chatgpt.com/docs/app-server.md`; generate schemas with `codex app-server generate-json-schema`.
- Claude Code headless: `https://code.claude.com/docs/en/headless`.
- alacritty_terminal: `https://docs.rs/alacritty_terminal`.
- gitoxide: `https://docs.rs/gix` (has a git2 → gix method map).
