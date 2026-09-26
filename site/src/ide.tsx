// InsyDE's interface, rebuilt in HTML from the desktop app's layout and tokens
// so the page can animate it. Names match the app's regions (top bar, sidebar,
// agent tabs, chat, right panel, terminals, status bar).
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { motion, useInView } from 'motion/react';

/** Renders `children` at a fixed desktop size (w × h) and scales it to the
 *  container's width, so the UI keeps its real proportions on any screen. */
export function Scaled({ w, h, children }: { w: number; h: number; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [k, setK] = useState(1);
  useLayoutEffect(() => {
    const el = ref.current!;
    const ro = new ResizeObserver(([e]) => setK(e.contentRect.width / w));
    ro.observe(el);
    return () => ro.disconnect();
  }, [w]);
  return (
    <div ref={ref} style={{ width: '100%', height: h * k, position: 'relative' }}>
      <div style={{ position: 'absolute', left: 0, top: 0, width: w, height: h, transform: `scale(${k})`, transformOrigin: '0 0' }}>{children}</div>
    </div>
  );
}

export function Ic({ n, s = 14, style }: { n: string; s?: number; style?: CSSProperties }) {
  return (
    <i
      className="ic"
      style={{ width: s, height: s, ['--src' as string]: `url(/icons/${n}.svg)`, ...style }}
    />
  );
}

export const AGENTS = [
  { key: 'claude', name: 'Claude Code', by: 'Anthropic', color: 'var(--claude)', fg: '#fff' },
  { key: 'codex', name: 'Codex', by: 'OpenAI', color: '#F2F2F0', fg: '#111' },
  { key: 'opencode', name: 'OpenCode', by: 'sst', color: '#2F2F2D', fg: '#F2F2F0' },
  { key: 'pi', name: 'Pi', by: 'pi.dev', color: '#7C5CD6', fg: '#fff' },
  { key: 'grok', name: 'Grok', by: 'xAI', color: '#101010', fg: '#fff' },
  { key: 'cursor', name: 'Cursor', by: 'Anysphere', color: '#1F1F1E', fg: '#F2F2F0' },
  { key: 'antigravity', name: 'Antigravity', by: 'Google', color: '#3B7DD8', fg: '#fff' },
] as const;
export type AgentKey = (typeof AGENTS)[number]['key'];

export function AgentLogo({ k, s = 14, color }: { k: AgentKey; s?: number; color?: string }) {
  return (
    <i
      className="ic"
      style={{ width: s, height: s, color, ['--src' as string]: `url(/agents/${k}.svg)` }}
    />
  );
}

export function Mg({ k, s = 18 }: { k: AgentKey; s?: number }) {
  const a = AGENTS.find((x) => x.key === k)!;
  return (
    <span className="mg" style={{ width: s, height: s, background: a.color, color: a.fg, boxShadow: 'inset 0 0 0 1px rgba(255,255,255,.08)' }}>
      <AgentLogo k={k} s={s * 0.6} />
    </span>
  );
}

/* ---------------------------------------------------------------- top bar */
export function TopBar({ side = 250, right = 300, compact = false }: { side?: number; right?: number; compact?: boolean }) {
  return (
    <div className="ide-top">
      <div className="col-side" style={{ width: side }}>
        <div className="tl"><i /><i /><i /></div>
        <div className="brand"><b>InsyDE</b><span>by Insyd</span></div>
      </div>
      <div className="col-mid">
        <span className="ghost"><Ic n="brain" s={15} style={{ color: 'var(--ink-2)' }} />Context<span className="n">1,313</span></span>
        {!compact && <span className="ghost muted"><Ic n="refresh" s={13} />Update</span>}
        {!compact && <span className="faint" style={{ fontSize: 11 }}>Updated 2m ago</span>}
        <span style={{ flex: 1 }} />
        <span className="muted" style={{ fontSize: 12, marginRight: 4 }}>3 agents</span>
        <span className="iconbtn"><Ic n="history" s={15} /></span>
        <span className="obtn"><Ic n="handoff" s={13} />Hand off</span>
      </div>
      {!compact && <div className="col-right" style={{ width: right }}>
        <span className="muted" style={{ fontSize: 12, fontWeight: 500, marginRight: 6 }}>$1.84</span>
        <span className="iconbtn"><Ic n="layout" s={15} /></span>
        <span className="iconbtn"><Ic n="sun" s={15} /></span>
        <span className="pbtn"><span><Ic n="play" s={11} />Run pnpm</span><i /><em><Ic n="chevron-down" s={11} /></em></span>
      </div>}
    </div>
  );
}

/* ---------------------------------------------------------------- sidebar */
const WORKTREES = [
  { b: 'main', s: '12m ago', star: true },
  { b: 'feat/checkout-flow', s: 'Claude Code · running', add: 214, rem: 38, live: true },
  { b: 'fix/session-expiry', s: 'Codex · needs review', add: 41, rem: 12, review: true },
  { b: 'chore/upgrade-vite', s: 'OpenCode · 3m ago', add: 18, rem: 22 },
];

export function Sidebar({ w = 250, active = 1, extra }: { w?: number; active?: number; extra?: ReactNode }) {
  return (
    <div className="ide-side" style={{ width: w }}>
      <div style={{ padding: '10px 10px 8px' }}>
        <div className="seg"><span className="on">Worktrees</span><span>Files</span><span>Search</span></div>
      </div>
      <div className="cap" style={{ marginTop: 6 }}>Projects</div>
      <div className="proj">
        <span className="mono-l">A</span>
        <div style={{ flex: 1 }}>
          <div style={{ fontWeight: 600 }}>acme-web</div>
          <div className="muted" style={{ fontSize: 11 }}>TypeScript · 4 worktrees</div>
        </div>
        <Ic n="plus" s={12} style={{ color: 'var(--ink-3)' }} />
      </div>
      {WORKTREES.map((w, i) => (
        <div key={w.b} className={`wt${i === active ? ' on' : ''}`}>
          <span className="idx">{i + 1}</span>
          <Ic n="branch" s={13} style={{ color: i === active ? 'var(--sel-text)' : 'var(--ink-3)', marginTop: 2 }} />
          <div style={{ minWidth: 0 }}>
            <div className="t">{w.b}{w.star && <span className="faint"> ★</span>}</div>
            <div className="s" style={w.review ? { color: 'var(--warn)' } : undefined}>
              {w.live && <span style={{ display: 'inline-block', width: 6, height: 6, borderRadius: 9, background: 'var(--ok)', marginRight: 6, animation: 'pulse 1.6s infinite' }} />}
              {w.s}
            </div>
          </div>
          {w.add !== undefined && <span className="d"><span className="add">+{w.add}</span> <span className="rem">−{w.rem}</span></span>}
        </div>
      ))}
      {extra}
      <div className="side-foot">
        <span className="iconbtn"><Ic n="settings" s={15} /></span>
        <span className="iconbtn"><Ic n="folder-plus" s={15} /></span>
        <div className="pages"><span className="dot" /><span className="cur">A</span><span className="dot" /><Ic n="plus" s={12} style={{ color: 'var(--ink-3)' }} /></div>
      </div>
    </div>
  );
}

/* ---------------------------------------------------------------- tabs + chat */
export function Tabs({ active = 0 }: { active?: number }) {
  const tabs: { k: AgentKey; t: string; live?: boolean }[] = [
    { k: 'claude', t: 'Checkout: Stripe flow', live: true },
    { k: 'codex', t: 'Write e2e tests' },
    { k: 'opencode', t: 'Review the diff' },
  ];
  return (
    <div className="tabs">
      {tabs.map((t, i) => (
        <div key={t.t} className={`tab${i === active ? ' on' : ''}`}>
          <Mg k={t.k} />
          <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{t.t}</span>
          {t.live ? <span className="live" /> : <span className="x">×</span>}
        </div>
      ))}
      <div className="addag"><Ic n="plus" s={12} />Agent<Ic n="chevron-down" s={11} style={{ color: 'var(--ink-3)', marginLeft: 6 }} /></div>
    </div>
  );
}

type ChatBeat =
  | { kind: 'user'; text: string }
  | { kind: 'worked'; text: string }
  | { kind: 'tool'; title: string; body: string; status: string }
  | { kind: 'text'; text: ReactNode };

const SCRIPT: ChatBeat[] = [
  { kind: 'user', text: 'Add Stripe checkout to the cart page. Keep the existing button styles.' },
  { kind: 'worked', text: 'Worked for 38s ›' },
  { kind: 'tool', title: 'Read  src/cart/CartPage.tsx', body: 'import { Button } from "@/ui/button"\nexport function CartPage() {', status: 'done' },
  { kind: 'tool', title: 'Edit  src/cart/checkout.ts', body: '+ const session = await stripe.checkout.sessions.create({\n+   mode: "payment", line_items: cart.toLineItems(),', status: '+86 −4' },
  { kind: 'tool', title: 'Run  pnpm test cart', body: '✓ 14 passed (1.2s)', status: 'passed' },
  { kind: 'text', text: <>Checkout now opens a Stripe session from <code>CartPage</code>. The button reuses <code>Button variant="primary"</code>, and the cart tests pass.</> },
];

/** The chat body. When `play` is set, the transcript streams in step by step. */
export function Chat({ play = true, compact = false }: { play?: boolean; compact?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const inView = useInView(ref, { amount: 0.3 });
  const [n, setN] = useState(play ? 0 : SCRIPT.length);
  useEffect(() => {
    if (!play || !inView) return;
    if (n >= SCRIPT.length) {
      const t = setTimeout(() => setN(0), 5200);
      return () => clearTimeout(t);
    }
    const t = setTimeout(() => setN((x) => x + 1), n === 0 ? 500 : 900);
    return () => clearTimeout(t);
  }, [n, inView, play]);
  const beats = SCRIPT.slice(0, n);
  return (
    <div ref={ref} style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
      <div className="chat" style={compact ? { padding: '14px 18px 0', gap: 10 } : undefined}>
        {beats.map((b, i) => (
          <motion.div key={i} initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.35, ease: 'easeOut' }}
            style={b.kind === 'user' ? { alignSelf: 'flex-end', maxWidth: '78%' } : undefined}>
            {b.kind === 'user' && <div className="bubble" style={{ maxWidth: '100%' }}>{b.text}</div>}
            {b.kind === 'worked' && <div className="worked">{b.text}</div>}
            {b.kind === 'tool' && (
              <div className="toolc">
                <div className="h">
                  <Ic n={b.title.startsWith('Run') ? 'running' : 'file'} s={12} style={{ color: 'var(--ink-3)' }} />
                  <span style={{ fontFamily: 'var(--ide-mono)', fontSize: 12 }}>{b.title}</span>
                  <span className="st" style={{ color: b.status === 'passed' ? 'var(--ok)' : 'var(--ink-3)' }}>{b.status}</span>
                </div>
                <div className="b" style={{ whiteSpace: 'pre' }}>{b.body}</div>
              </div>
            )}
            {b.kind === 'text' && <p>{b.text}</p>}
          </motion.div>
        ))}
        {n < SCRIPT.length && n > 0 && <div className="worked"><span className="caret" /></div>}
      </div>
      <div className="composer" style={compact ? { margin: '10px 14px 12px' } : undefined}>
        <div className="ph">Ask for changes, send follow-ups, or paste an error…</div>
        <div className="row">
          <span>Claude Code ⌄</span>
          {!compact && <span>Auto-approve edits ⌄</span>}
          {!compact && <span className="tag">feat/checkout-flow</span>}
          <span className="tag brain">● Brain context</span>
          <span className="send"><Ic n="send" s={12} /></span>
        </div>
      </div>
    </div>
  );
}

/* ---------------------------------------------------------------- right panel */
export function RightPanel({ w = 300, tab = 1 }: { w?: number; tab?: number }) {
  return (
    <div className="ide-right" style={{ width: w }}>
      <div style={{ padding: '10px 10px 8px', display: 'flex', gap: 8, alignItems: 'center' }}>
        <div className="seg" style={{ flex: 1 }}>
          {['Checks', 'Diff', 'Editor'].map((t, i) => <span key={t} className={i === tab ? 'on' : ''}>{t}</span>)}
        </div>
        <Ic n="popout" s={13} style={{ color: 'var(--ink-3)' }} />
      </div>
      <DiffBody />
    </div>
  );
}

export function DiffBody() {
  return (
    <>
      <div className="cap" style={{ padding: '4px 12px 6px' }}>3 files changed · <span className="add">+214</span> <span className="rem">−38</span></div>
      <div className="diff-file on"><Ic n="file" s={12} style={{ color: 'var(--ink-3)' }} />checkout.ts<span className="d"><span className="add">+86</span> <span className="rem">−4</span></span></div>
      <div className="diff-file"><Ic n="file" s={12} style={{ color: 'var(--ink-3)' }} />CartPage.tsx<span className="d"><span className="add">+22</span> <span className="rem">−9</span></span></div>
      <div className="diff-file"><Ic n="file" s={12} style={{ color: 'var(--ink-3)' }} />cart.test.ts<span className="d"><span className="add">+106</span> <span className="rem">−25</span></span></div>
      <div className="patch" style={{ marginTop: 8 }}>
        <div className="h">@@ -12,6 +12,19 @@ export async function checkout(cart)</div>
        <div>{'  const items = cart.toLineItems()'}</div>
        <div className="r">{'- return redirect("/pay")'}</div>
        <div className="a">{'+ const session = await stripe.checkout'}</div>
        <div className="a">{'+   .sessions.create({ mode: "payment",'}</div>
        <div className="a">{'+     line_items: items,'}</div>
        <div className="a">{'+     success_url: url("/thanks") })'}</div>
        <div className="a">{'+ return redirect(session.url)'}</div>
        <div>{'}'}</div>
      </div>
    </>
  );
}

/* ---------------------------------------------------------------- terminals */
type Line = { t: string; c?: string };
const PANES: { title: string; dot: string; lines: Line[] }[] = [
  { title: 'pnpm dev', dot: 'var(--blue)', lines: [
    { t: '$ pnpm dev', c: 'p' }, { t: '  VITE v8.3  ready in 212 ms', c: 'g' }, { t: '  ➜  Local:   http://localhost:5173/', c: 'b' }, { t: '  hmr update /src/cart/CartPage.tsx' },
  ] },
  { title: 'pnpm test --watch', dot: 'var(--amber)', lines: [
    { t: '$ pnpm test --watch', c: 'p' }, { t: ' ✓ cart/checkout.test.ts (9)', c: 'g' }, { t: ' ✓ cart/CartPage.test.tsx (5)', c: 'g' }, { t: ' Tests  14 passed', c: 'g' },
  ] },
  { title: 'zsh', dot: 'var(--green)', lines: [
    { t: '$ git status -sb', c: 'p' }, { t: '## feat/checkout-flow', c: 'y' }, { t: ' M src/cart/CartPage.tsx' }, { t: ' M src/cart/checkout.ts' },
  ] },
];

export function Terminals({ h = 190, animate = true, panes = PANES, fill = false }: { h?: number; animate?: boolean; panes?: typeof PANES; fill?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const inView = useInView(ref, { amount: 0.3, once: true });
  return (
    <div ref={ref} className="ide-bottom" style={fill ? { flex: 1, minHeight: 0 } : { height: h }}>
      <div className="bot-head">
        <b style={{ fontWeight: 600 }}>Terminals</b>
        <span className="muted" style={{ fontSize: 12 }}>{panes.length} sessions · 0 problems</span>
        <span style={{ flex: 1 }} />
        <div className="seg" style={{ fontSize: 12 }}><span className="on">Terminals</span><span>Logs</span><span>Problems</span></div>
      </div>
      <div className="panes">
        {panes.map((p, pi) => (
          <div key={p.title} className="pane">
            <div className="pane-h"><i style={{ background: p.dot }} /><b style={{ fontWeight: 500 }}>{p.title}</b><span style={{ flex: 1 }} /><span className="faint" style={{ fontSize: 11 }}>Pop out</span></div>
            <div className="term">
              {p.lines.map((l, i) => (
                <motion.div key={i} className={l.c} initial={animate ? { opacity: 0 } : false}
                  animate={inView || !animate ? { opacity: 1 } : {}} transition={{ delay: 0.25 + pi * 0.35 + i * 0.28, duration: 0.2 }}>
                  {l.t}
                </motion.div>
              ))}
              <div><span className="p">$ </span><span className="caret" /></div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export function StatusBar() {
  return (
    <div className="status">
      <b>feat/checkout-flow</b><span>↑3 ↓0</span><span>PR #482 draft</span><span>Agents: 1 running</span>
      <span style={{ marginLeft: 'auto' }}>UTF-8 · TypeScript</span>
    </div>
  );
}

/** The whole app window, as in the desktop build. */
export function IdeWindow({ className = '' }: { className?: string }) {
  return (
    <div className={`win ${className}`} style={{ height: '100%' }}>
      <TopBar />
      <div className="ide-body">
        <Sidebar />
        <div className="ide-center">
          <div className="sheet">
            <Tabs />
            <Chat />
          </div>
        </div>
        <RightPanel />
      </div>
      <Terminals h={170} />
      <StatusBar />
    </div>
  );
}

/* ---------------------------------------------------------------- popovers */
export function RunMenu() {
  return (
    <div className="pop" style={{ width: 300 }}>
      <div className="cap2">Run in a new terminal</div>
      <div className="row-i hl"><Ic n="play" s={9} style={{ color: 'var(--ink-3)' }} /><span className="mono" style={{ fontFamily: 'var(--ide-mono)', fontSize: 12 }}>pnpm run dev</span><span className="r">Run button</span></div>
      <div className="row-i"><Ic n="play" s={9} style={{ color: 'var(--ink-3)' }} /><span style={{ fontFamily: 'var(--ide-mono)', fontSize: 12 }}>pnpm test --watch</span></div>
      <div className="row-i"><Ic n="play" s={9} style={{ color: 'var(--ink-3)' }} /><span style={{ fontFamily: 'var(--ide-mono)', fontSize: 12 }}>docker compose up db</span></div>
      <div className="sepl" />
      <div style={{ padding: '4px 4px 2px' }}>
        <div style={{ height: 28, border: '1px solid var(--accent)', borderRadius: 6, display: 'flex', alignItems: 'center', padding: '0 8px', fontSize: 12, color: 'var(--ink-3)', boxShadow: '0 0 0 3px rgba(61,116,232,.2)' }}>Any command, e.g. pnpm lint<span className="caret" style={{ height: 12, width: 1.5, marginLeft: 2 }} /></div>
        <div style={{ display: 'flex', fontSize: 11, color: 'var(--ink-3)', padding: '6px 4px 2px' }}>Enter runs it and saves it here<span style={{ marginLeft: 'auto' }}>Edit in Settings</span></div>
      </div>
    </div>
  );
}

export function FileMenu() {
  const rows = [['Open'], ['Open in New Tab'], ['Open in Agent Area'], null, ['Reveal in Finder'], ['Open in Terminal'], null, ['Copy'], ['Copy Path'], ['Copy Relative Path'], null, ['Rename…'], ['Duplicate'], null, ['Move to Trash']];
  return (
    <div className="pop" style={{ width: 210 }}>
      <div className="cap2">checkout.ts</div>
      {rows.map((r, i) => r === null ? <div key={i} className="sepl" /> :
        <div key={i} className={`row-i${i === 1 ? ' hl' : ''}${r[0] === 'Move to Trash' ? ' danger' : ''}`} style={{ height: 27 }}>{r[0]}</div>)}
    </div>
  );
}

export function HandoffPop() {
  const opts: [string, string, string, boolean][] = [
    ['Conversation summary', 'Goals, decisions and what was tried', '0.9k', true],
    ['Changed files & diff', '3 files · +214 −38', '3.3k', true],
    ['Terminal & test state', 'pnpm test · last 200 lines', '1.1k', true],
    ['Open tasks', '2 open plan items', '0.2k', false],
    ['Project Brain', 'Linked notes for this task', '2.4k', true],
  ];
  const [on, setOn] = useState(opts.map((o) => o[3]));
  const agents: AgentKey[] = ['claude', 'codex', 'opencode', 'pi', 'grok', 'cursor'];
  return (
    <div className="pop" style={{ width: 360, padding: 0 }}>
      <div style={{ padding: '14px 16px 10px' }}>
        <div style={{ fontWeight: 600 }}>Hand off session</div>
        <div className="muted" style={{ fontSize: 12, marginTop: 2 }}>Start a fresh agent with this session's context carried over.</div>
      </div>
      <div style={{ padding: '0 12px' }}>
        <div className="cap2" style={{ padding: '4px 4px 6px' }}>Continue with</div>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 4 }}>
          {agents.map((k, i) => {
            const a = AGENTS.find((x) => x.key === k)!;
            return (
              <div key={k} className="row-i" style={{ height: 32, border: `1px solid ${i === 1 ? 'var(--sel-chip)' : 'var(--line-soft)'}`, background: i === 1 ? 'var(--sel-bg)' : undefined, fontSize: 12 }}>
                <Mg k={k} s={16} />{a.name}
              </div>
            );
          })}
        </div>
        <div className="cap2" style={{ padding: '12px 4px 4px' }}>Carry over</div>
        {opts.map((o, i) => (
          <div key={o[0]} className="row-i" style={{ height: 44, cursor: 'pointer' }} onClick={() => setOn((v) => v.map((x, j) => (j === i ? !x : x)))}>
            <span className={`check${on[i] ? ' on' : ''}`}>{on[i] && <Ic n="check" s={10} style={{ color: '#fff' }} />}</span>
            <div style={{ flex: 1 }}><div style={{ fontSize: 12.5 }}>{o[0]}</div><div className="muted" style={{ fontSize: 11 }}>{o[1]}</div></div>
            <span className="muted" style={{ fontSize: 11 }}>{o[2]}</span>
          </div>
        ))}
      </div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: 12, borderTop: '1px solid var(--line)', marginTop: 8 }}>
        <span className="muted" style={{ fontSize: 11.5 }}>{(opts.reduce((s, o, i) => s + (on[i] ? parseFloat(o[2]) : 0), 0)).toFixed(1)}k → fresh window</span>
        <span style={{ flex: 1 }} />
        <span className="obtn" style={{ height: 28 }}>Cancel</span>
        <span className="pbtn" style={{ height: 28 }}><span>Hand off to Codex</span></span>
      </div>
    </div>
  );
}
