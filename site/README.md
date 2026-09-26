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

Look: quiet on purpose. One static gradient, flat panels, serif-italic accents, short one-time reveals.
No blur filters, no looping page animations and no scroll hijacking, so it stays smooth on laptops.

Motion: `motion` for reveals and scroll-linked transforms, `lenis` for smooth scrolling; both respect
`prefers-reduced-motion`.
