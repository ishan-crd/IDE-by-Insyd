# site — the InsyDE landing page

React + Vite, one package in this repo's pnpm workspace (the Rust app is the Cargo workspace in `crates/`).

```
pnpm install         # at the repo root (pnpm workspace)
pnpm site            # dev server
pnpm site:build      # site/dist
```

## Deploying (Vercel → ide.insyd.in)

- Import the repo and set **Root Directory** to `site`. `site/vercel.json` sets the rest: Vite, `pnpm install
  --frozen-lockfile`, `pnpm build`, output `dist`, long-lived caching for hashed assets.
- Node 20.19+ (Vite 8). The pnpm version is pinned in the root `package.json` (`packageManager`).
- Add the domain `ide.insyd.in` in Project → Domains, then a DNS `CNAME ide → cname.vercel-dns.com`
  at the insyd.in DNS provider.
- `index.html` has the canonical URL, Open Graph and Twitter tags pointing at `https://ide.insyd.in`;
  `public/og.png` is the share image (a 1200×630 capture of the hero). `robots.txt` and `sitemap.xml` are in `public/`.

The product shots are not screenshots: `src/ide.tsx` rebuilds the app's regions (top bar, sidebar, agent
tabs, chat, diff, terminals, popovers) in HTML with the app's own tokens and icons (`public/icons` is
`assets/icons`, `public/agents` is `assets/icons/agents`), so they stay sharp and can animate. `Scaled`
renders them at desktop size and scales to fit. If the app's UI changes, update these components with it.

Look: quiet on purpose. One static gradient, flat panels, serif-italic accents, short one-time reveals.
No blur filters, no looping page animations and no scroll hijacking, so it stays smooth on laptops.

Motion: `motion` for one-time reveals; `prefers-reduced-motion` turns them off.
