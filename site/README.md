# site — the InsyDE landing page

React + Vite, one package in this repo's npm workspace (the Rust app is the Cargo workspace in `crates/`).

```
npm install          # at the repo root
npm run site         # dev server
npm run site:build   # site/dist
```

The product shots are not screenshots: `src/ide.tsx` rebuilds the app's regions (top bar, sidebar, agent
tabs, chat, diff, terminals, popovers) in HTML with the app's own tokens and icons (`public/icons` is
`assets/icons`, `public/agents` is `assets/icons/agents`), so they stay sharp and can animate. `Scaled`
renders them at desktop size and scales to fit. If the app's UI changes, update these components with it.

Look: a drifting aurora behind glass panels, serif-italic accents (Instrument Serif), and the app shown in its
real Glass mode (`IdeWindow glass`) on a desktop wallpaper.

Motion: `motion` for reveals and scroll-linked transforms, `lenis` for smooth scrolling; both respect
`prefers-reduced-motion`.
