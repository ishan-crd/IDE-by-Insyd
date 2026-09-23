# design/ — put the product design here

The implementing agent reads this folder as the single source of truth for the UI.

Drop in:
- `tokens.json` (colors light/dark, type scale, spacing, radii, shadows, motion) — or the export from your design tool and Opus will derive it.
- `screens/` — one image or HTML per screen and state (empty workspace, sidebar with worktrees, chat with tool calls + permission prompt, terminal, diff/review, PR panel, palette, settings, PiP, flow mode).
- `components/` — buttons, tabs, inputs, cards, chat blocks, diff rows, terminal chrome, badges.
- Your notes: keymap, naming (what you call workspace / project / worktree / tab), and anything the design implies but does not show.

If the design was made with Claude Design, export it (HTML/assets) and place the export here; a URL alone is not enough for the implementing agent to work offline.
