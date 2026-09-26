import { useEffect, useRef, useState, type ReactNode } from 'react';
import { AnimatePresence, animate, motion, useInView, useMotionValue, useScroll, useSpring, useTransform } from 'motion/react';
import {
  AGENTS, AgentLogo, Chat, DiffBody, FileMenu, HandoffPop, Ic, IdeWindow, Mg, RunMenu, Scaled, Sidebar, Tabs, Terminals, TopBar,
  type AgentKey,
} from './ide';

const GITHUB = 'https://github.com/ishan-crd/IDE-by-Insyd';
const DOWNLOAD = `${GITHUB}/releases`;

const ease = [0.22, 1, 0.36, 1] as const;

function Reveal({ children, delay = 0, y = 24, className }: { children: ReactNode; delay?: number; y?: number; className?: string }) {
  return (
    <motion.div className={className} initial={{ opacity: 0, y, filter: 'blur(6px)' }} whileInView={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
      viewport={{ once: true, amount: 0.25 }} transition={{ duration: 0.8, delay, ease }}>
      {children}
    </motion.div>
  );
}

/* ------------------------------------------------------------------ nav */
function Nav() {
  const [scrolled, setScrolled] = useState(false);
  useEffect(() => {
    const on = () => setScrolled(window.scrollY > 12);
    on();
    window.addEventListener('scroll', on, { passive: true });
    return () => window.removeEventListener('scroll', on);
  }, []);
  return (
    <nav className={`nav${scrolled ? ' scrolled' : ''}`}>
      <div className="wrap nav-in">
        <a href="#" className="logo"><img src="/icon.svg" alt="" />InsyDE <small>by Insyd</small></a>
        <div className="nav-links">
          <a href="#how">How it works</a><a href="#features">Features</a><a href="#agents">Agents</a><a href="#native">Native</a>
        </div>
        <div className="nav-right">
          <a className="btn btn-ghost btn-sm" href={GITHUB} target="_blank" rel="noreferrer">GitHub</a>
          <a className="btn btn-primary btn-sm" href={DOWNLOAD} target="_blank" rel="noreferrer"><AppleMark />Download</a>
        </div>
      </div>
    </nav>
  );
}

function AppleMark({ s = 15 }: { s?: number }) {
  return (
    <svg width={s} height={s} viewBox="0 0 24 24" fill="currentColor" aria-hidden>
      <path d="M16.37 12.64c-.02-2.16 1.77-3.2 1.85-3.25-1-1.47-2.57-1.67-3.13-1.7-1.33-.13-2.6.79-3.28.79-.68 0-1.72-.77-2.83-.75-1.46.02-2.8.85-3.55 2.15-1.51 2.63-.39 6.52 1.09 8.65.72 1.04 1.58 2.21 2.71 2.17 1.09-.04 1.5-.7 2.81-.7 1.31 0 1.68.7 2.83.68 1.17-.02 1.91-1.06 2.62-2.11.83-1.21 1.17-2.38 1.19-2.44-.03-.01-2.29-.88-2.31-3.49zM14.23 6.28c.6-.73 1-1.73.89-2.74-.86.03-1.9.57-2.52 1.3-.55.64-1.04 1.66-.91 2.64.96.07 1.94-.49 2.54-1.2z" />
    </svg>
  );
}

/* ------------------------------------------------------------------ hero */
const TILE_SPOTS: { k: AgentKey; x: string; y: number; r: number; d: number }[] = [
  { k: 'claude', x: '9%', y: 190, r: -8, d: 0.9 },
  { k: 'opencode', x: '26%', y: 96, r: 6, d: 0.5 },
  { k: 'pi', x: '17%', y: 470, r: 7, d: 1.2 },
  { k: 'codex', x: '84%', y: 196, r: 8, d: 0.8 },
  { k: 'antigravity', x: '69%', y: 92, r: -6, d: 0.4 },
  { k: 'grok', x: '79%', y: 470, r: -7, d: 1.1 },
];

function Hero() {
  const ref = useRef<HTMLDivElement>(null);
  const { scrollYProgress } = useScroll({ target: ref, offset: ['start start', 'end start'] });
  const rot = useTransform(scrollYProgress, [0, 0.35], [22, 0]);
  const scale = useTransform(scrollYProgress, [0, 0.35], [0.9, 1]);
  const y = useTransform(scrollYProgress, [0, 0.35], [40, 0]);
  const tilesY = useTransform(scrollYProgress, [0, 0.5], [0, -160]);
  const tilesO = useTransform(scrollYProgress, [0, 0.3], [1, 0]);
  const words = ['The', 'native', 'IDE', 'for'];
  return (
    <section className="hero" ref={ref}>
      <div className="grid-bg" />
      <div className="glow" style={{ width: 620, height: 420, left: '50%', top: 40, transform: 'translateX(-50%)', background: 'radial-gradient(closest-side, rgba(61,116,232,.35), transparent)' }} />
      <motion.div className="tiles" style={{ y: tilesY, opacity: tilesO }}>
        {TILE_SPOTS.map((t) => {
          const a = AGENTS.find((x) => x.key === t.k)!;
          return (
            <motion.div key={t.k} className="tile" style={{ left: t.x, top: t.y, rotate: t.r }}
              initial={{ opacity: 0, scale: 0.6, y: 20 }} animate={{ opacity: 1, scale: 1, y: [0, -10, 0] }}
              transition={{ opacity: { delay: t.d, duration: 0.6 }, scale: { delay: t.d, duration: 0.8, ease }, y: { delay: t.d + 0.8, duration: 5 + t.d * 2, repeat: Infinity, ease: 'easeInOut' } }}>
              <AgentLogo k={t.k} s={36} color={t.k === 'claude' ? 'var(--claude)' : t.k === 'antigravity' ? '#7FA6F5' : a.key === 'pi' ? '#B7A3F0' : '#F2F2F0'} />
            </motion.div>
          );
        })}
      </motion.div>

      <div className="wrap hero-copy">
        <motion.div initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.6, ease }}>
          <span className="pill"><b>New</b>Hand off a session to another agent<span className="long">, context included</span></span>
        </motion.div>
        <h1 className="h1">
          {words.map((w, i) => (
            <motion.span key={w} style={{ display: 'inline-block', marginRight: '.24em' }} initial={{ opacity: 0, y: 30, filter: 'blur(10px)' }}
              animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }} transition={{ duration: 0.9, delay: 0.1 + i * 0.08, ease }}>{w}</motion.span>
          ))}
          <br />
          <motion.span className="dim" style={{ display: 'inline-block' }} initial={{ opacity: 0, y: 30, filter: 'blur(10px)' }}
            animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }} transition={{ duration: 0.9, delay: 0.45, ease }}>coding agents.</motion.span>
        </h1>
        <motion.p className="lede" initial={{ opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.8, delay: 0.6, ease }}>
          Run <strong>Claude Code, Codex, OpenCode, Pi, Grok, Cursor and Antigravity</strong> side by side. Each task gets its own git worktree,
          terminals and diff. Written in Rust, drawn on the GPU, no Electron.
        </motion.p>
        <motion.div className="ctas" initial={{ opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.8, delay: 0.72, ease }}>
          <a className="btn btn-primary" href={DOWNLOAD} target="_blank" rel="noreferrer"><AppleMark />Download for macOS</a>
          <a className="btn btn-ghost" href={GITHUB} target="_blank" rel="noreferrer">View source <span style={{ opacity: .6 }}>↗</span></a>
        </motion.div>
        <motion.div className="hint" initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 1 }}>
          Away from your desk? <code>insy serve</code> puts it in your phone's browser.
        </motion.div>
      </div>

      <div className="wrap stage">
        <div className="stage-glow" />
        <motion.div style={{ rotateX: rot, scale, y, transformOrigin: '50% 0%' }}
          initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 1.2, delay: 0.5 }}>
          <Scaled w={1360} h={820}><IdeWindow /></Scaled>
        </motion.div>
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ agents strip */
function AgentStrip() {
  const row = [...AGENTS, ...AGENTS];
  return (
    <section className="agents" id="agents">
      <div className="wrap agents-title">Bring the agents you already pay for. InsyDE speaks their protocols.</div>
      <div className="marquee">
        <div className="marquee-track">
          {row.map((a, i) => (
            <div key={i} className="chip">
              <Mg k={a.key} s={30} />
              <span>{a.name}</span><em>{a.by}</em>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ story (sticky) */
function Frame({ children, dim }: { children: ReactNode; dim?: boolean }) {
  return (
    <div className="win" style={{ height: '100%' }}>
      <TopBar side={210} compact />
      <div className="ide-body" style={dim ? { filter: 'brightness(.55)' } : undefined}>{children}</div>
    </div>
  );
}

function VisualWorktrees() {
  const [typed, setTyped] = useState('');
  const full = 'fix/session-expiry';
  useEffect(() => {
    let i = 0;
    const t = setInterval(() => { i = (i + 1) % (full.length + 14); setTyped(full.slice(0, Math.min(i, full.length))); }, 90);
    return () => clearInterval(t);
  }, []);
  return (
    <Frame>
      <Sidebar w={270} active={1} extra={
        <div style={{ margin: '10px 12px 0', height: 30, border: '1px solid var(--accent)', borderRadius: 6, display: 'flex', alignItems: 'center', padding: '0 9px', fontSize: 12.5, boxShadow: '0 0 0 3px rgba(61,116,232,.18)' }}>
          {typed}<span className="caret" style={{ width: 1.5, height: 13, marginLeft: 1 }} />
        </div>} />
      <div className="ide-center"><div className="sheet"><Tabs /><Terminals fill animate={false} panes={[{ title: 'zsh', dot: 'var(--green)', lines: [
        { t: '$ insy worktree create "fix/session-expiry"', c: 'p' },
        { t: 'Preparing worktree (new branch fix/session-expiry)' },
        { t: 'copied .env, .env.local', c: 'g' },
        { t: '$ pnpm install', c: 'p' }, { t: 'Done in 3.1s', c: 'g' },
      ] }]} /></div></div>
    </Frame>
  );
}

function VisualChat() {
  return (
    <Frame>
      <div className="ide-center"><div className="sheet"><Tabs /><Chat compact /></div></div>
    </Frame>
  );
}

function VisualDiff() {
  return (
    <Frame>
      <div className="ide-center" style={{ flex: '0 0 50%' }}><div className="sheet"><Tabs active={2} /><Chat play={false} compact /></div></div>
      <div className="ide-right" style={{ flex: 1 }}>
        <div style={{ padding: '10px 10px 8px' }}><div className="seg"><span>Checks</span><span className="on">Diff</span><span>Editor</span></div></div>
        <DiffBody />
      </div>
    </Frame>
  );
}

function VisualHandoff() {
  return (
    <div style={{ position: 'relative', height: '100%' }}>
      <Frame dim><div className="ide-center"><div className="sheet"><Tabs /><Chat play={false} compact /></div></div></Frame>
      <motion.div style={{ position: 'absolute', right: '6%', top: '11%' }} initial={{ opacity: 0, y: 12, scale: 0.97 }} animate={{ opacity: 1, y: 0, scale: 1 }} transition={{ duration: 0.5, delay: 0.15, ease }}>
        <HandoffPop />
      </motion.div>
    </div>
  );
}

const STEPS = [
  { t: 'One task, one worktree.', p: 'Type a task and InsyDE branches it, creates a git worktree, copies your .env files and runs your setup command. Agents never trip over each other.',
    li: ['Worktree ready in about 0.1 s', 'Diff stats and status for every branch in the sidebar', 'Local repos or any machine over SSH'], v: <VisualWorktrees /> },
  { t: 'Real agents, real protocols.', p: 'Chats run over the Agent Client Protocol, so you get tool calls, plans, permission prompts and cost, not a scraped terminal. Prefer the agent’s own UI? Open it in a terminal tab.',
    li: ['Streaming markdown and tool-call cards', 'Ask, auto-approve edits, or skip prompts', 'Transcripts are saved before they are shown'], v: <VisualChat /> },
  { t: 'Review before it ships.', p: 'Every change lands in the Diff panel with per-file stats. Comment on a line and it goes straight back to the agent. Checks and the PR live next to it.',
    li: ['Line comments become agent instructions', 'CI checks with re-run', 'Create or open the PR in one click'], v: <VisualDiff /> },
  { t: 'Hand off, keep the context.', p: 'Context window filling up, or want a second opinion? Start a fresh agent with the summary, diff, terminal state and brain notes carried over, and see the token cost first.',
    li: ['Pick exactly what carries over', 'Any agent to any agent', 'Suggested automatically as context fills'], v: <VisualHandoff /> },
];

function Story() {
  const [active, setActive] = useState(0);
  const refs = useRef<(HTMLDivElement | null)[]>([]);
  useEffect(() => {
    const io = new IntersectionObserver((es) => {
      for (const e of es) if (e.isIntersecting) setActive(Number((e.target as HTMLElement).dataset.i));
    }, { rootMargin: '-45% 0px -45% 0px' });
    refs.current.forEach((r) => r && io.observe(r));
    return () => io.disconnect();
  }, []);
  return (
    <section className="sec" id="how" style={{ paddingBottom: 60 }}>
      <div className="wrap">
        <Reveal><span className="eyebrow">How it works</span></Reveal>
        <Reveal delay={0.05}><h2 className="h2">A team of agents, <span className="dim">one calm window.</span></h2></Reveal>
        <div className="story">
          <div className="story-steps">
            {STEPS.map((s, i) => (
              <div key={s.t} ref={(el) => { refs.current[i] = el; }} data-i={i} className={`step${i === active ? ' on' : ''}`}>
                <div className="step-n">0{i + 1}</div>
                <h3>{s.t}</h3>
                <p>{s.p}</p>
                <ul>{s.li.map((l) => <li key={l}>{l}</li>)}</ul>
                <div className="step-visual"><Scaled w={700} h={600}>{s.v}</Scaled></div>
              </div>
            ))}
          </div>
          <div className="story-stage">
            <div className="story-frame">
              <AnimatePresence mode="popLayout">
                <motion.div key={active} initial={{ opacity: 0, y: 30, scale: 0.97 }} animate={{ opacity: 1, y: 0, scale: 1 }}
                  exit={{ opacity: 0, y: -30, scale: 0.97 }} transition={{ duration: 0.55, ease }}>
                  <Scaled w={700} h={600}>{STEPS[active].v}</Scaled>
                </motion.div>
              </AnimatePresence>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ bento */
function Card({ className = '', title, body, children }: { className?: string; title: string; body: string; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  return (
    <Reveal className={className}>
      <div ref={ref} className="card" style={{ height: '100%' }}
        onMouseMove={(e) => {
          const r = ref.current!.getBoundingClientRect();
          ref.current!.style.setProperty('--mx', `${e.clientX - r.left}px`);
          ref.current!.style.setProperty('--my', `${e.clientY - r.top}px`);
        }}>
        <h4>{title}</h4>
        <p>{body}</p>
        <div className="card-art">{children}</div>
      </div>
    </Reveal>
  );
}

function BrainGraph() {
  const ref = useRef<SVGSVGElement>(null);
  const inView = useInView(ref, { amount: 0.4, once: true });
  const nodes = [
    { x: 200, y: 110, r: 16, c: 'var(--blue)', l: 'checkout' }, { x: 90, y: 60, r: 9, c: 'var(--green)', l: 'CartPage' },
    { x: 330, y: 55, r: 10, c: 'var(--red)', l: 'stripe' }, { x: 80, y: 175, r: 8, c: 'var(--amber)', l: 'ADR-12' },
    { x: 330, y: 180, r: 11, c: 'var(--purple)', l: '/api/pay' }, { x: 470, y: 110, r: 8, c: 'var(--green)', l: 'session.ts' },
    { x: 200, y: 215, r: 7, c: 'var(--ink-4)', l: 'PR #431' }, { x: 470, y: 205, r: 6, c: 'var(--ink-4)', l: 'conventions' },
    { x: 560, y: 60, r: 7, c: 'var(--blue)', l: 'auth' },
  ];
  const edges = [[0, 1], [0, 2], [0, 3], [0, 4], [4, 5], [0, 6], [5, 7], [5, 8], [2, 4], [1, 3]];
  return (
    <svg ref={ref} viewBox="0 0 620 250" style={{ width: '100%', height: '100%', overflow: 'visible' }}>
      {edges.map(([a, b], i) => (
        <motion.line key={i} x1={nodes[a].x} y1={nodes[a].y} x2={nodes[b].x} y2={nodes[b].y} stroke="rgba(255,255,255,.14)" strokeWidth={1.2}
          initial={{ pathLength: 0 }} animate={inView ? { pathLength: 1 } : {}} transition={{ delay: 0.2 + i * 0.08, duration: 0.6 }} />
      ))}
      {nodes.map((n, i) => (
        <motion.g key={i} initial={{ opacity: 0, scale: 0.4 }} animate={inView ? { opacity: 1, scale: 1 } : {}} transition={{ delay: 0.1 + i * 0.07, duration: 0.5, ease }}
          style={{ transformOrigin: `${n.x}px ${n.y}px` }}>
          <circle cx={n.x} cy={n.y} r={n.r + 8} fill={n.c} opacity={0.12} />
          <circle cx={n.x} cy={n.y} r={n.r} fill={n.c} />
          <text x={n.x} y={n.y + n.r + 16} textAnchor="middle" fontSize="11" fill="var(--ink-3)" fontFamily="var(--ide-mono)">{n.l}</text>
        </motion.g>
      ))}
    </svg>
  );
}

function Phone() {
  return (
    <div style={{ display: 'flex', gap: 22, alignItems: 'center', justifyContent: 'center', height: '100%' }}>
      <motion.div initial={{ y: 30, rotate: -4, opacity: 0 }} whileInView={{ y: 0, rotate: -4, opacity: 1 }} viewport={{ once: true }} transition={{ duration: 0.9, ease }}
        style={{ width: 170, height: 300, borderRadius: 30, border: '6px solid #2b2b29', background: 'var(--ground)', boxShadow: '0 30px 60px -20px rgba(0,0,0,.8)', overflow: 'hidden', display: 'flex', flexDirection: 'column', font: '11px/1.4 var(--ide-font)' }}>
        <div style={{ height: 34, display: 'flex', alignItems: 'center', gap: 6, padding: '0 10px', borderBottom: '1px solid var(--line)', background: 'var(--panel)' }}>
          <Mg k="claude" s={14} /><b style={{ fontSize: 11 }}>Checkout</b><span style={{ marginLeft: 'auto', width: 6, height: 6, borderRadius: 9, background: 'var(--ok)' }} />
        </div>
        <div style={{ flex: 1, padding: 9, display: 'flex', flexDirection: 'column', gap: 7 }}>
          <div className="bubble" style={{ padding: '6px 8px', fontSize: 10.5, maxWidth: '85%' }}>Tests green?</div>
          <div className="toolc"><div className="h" style={{ fontSize: 10, padding: '5px 7px' }}>Run pnpm test<span className="st" style={{ color: 'var(--ok)' }}>passed</span></div></div>
          <div style={{ fontSize: 10.5, color: 'var(--ink-2)' }}>All 14 pass. Want me to open the PR?</div>
        </div>
        <div style={{ margin: 8, height: 30, borderRadius: 8, border: '1px solid var(--line)', background: 'var(--panel)', display: 'flex', alignItems: 'center', padding: '0 9px', color: 'var(--ink-3)', fontSize: 10.5 }}>Reply…</div>
      </motion.div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 10, alignItems: 'center' }}>
        <QR />
        <span className="muted" style={{ fontSize: 12 }}>Scan to pair</span>
      </div>
    </div>
  );
}

function QR() {
  // A decorative code pattern (not a real pairing link).
  const n = 21;
  const cells: boolean[] = [];
  let s = 7;
  for (let i = 0; i < n * n; i++) { s = (s * 9301 + 49297) % 233280; cells.push(s / 233280 > 0.52); }
  const finder = (x: number, y: number) => (x < 7 && y < 7) || (x >= n - 7 && y < 7) || (x < 7 && y >= n - 7);
  return (
    <svg viewBox={`0 0 ${n} ${n}`} width={116} height={116} style={{ background: '#fff', padding: 8, borderRadius: 12 }} shapeRendering="crispEdges">
      {cells.map((on, i) => {
        const x = i % n, y = Math.floor(i / n);
        if (finder(x, y)) return null;
        return on ? <rect key={i} x={x} y={y} width={1} height={1} fill="#111" /> : null;
      })}
      {[[0, 0], [n - 7, 0], [0, n - 7]].map(([x, y]) => (
        <g key={`${x}${y}`}><rect x={x} y={y} width={7} height={7} fill="#111" /><rect x={x + 1} y={y + 1} width={5} height={5} fill="#fff" /><rect x={x + 2} y={y + 2} width={3} height={3} fill="#111" /></g>
      ))}
    </svg>
  );
}

function SettingsMini() {
  const [glass, setGlass] = useState(true);
  const [n, setN] = useState(3);
  const Row = ({ t, d, c, first }: { t: string; d: string; c: ReactNode; first?: boolean }) => (
    <div style={{ display: 'flex', alignItems: 'center', gap: 16, padding: '11px 14px', borderTop: first ? 0 : '1px solid var(--line-soft)' }}>
      <div style={{ flex: 1 }}><div style={{ fontWeight: 500 }}>{t}</div><div className="muted" style={{ fontSize: 11.5 }}>{d}</div></div>{c}
    </div>
  );
  return (
    <div style={{ border: '1px solid var(--line)', borderRadius: 10, background: 'var(--panel)', font: '13px/1.4 var(--ide-font)' }}>
      <Row first t="Glass" d="See your desktop through the chrome." c={<span className={`switch${glass ? ' on' : ''}`} style={{ cursor: 'pointer' }} onClick={() => setGlass(!glass)} />} />
      <Row t="Terminals per worktree" d="Opened side by side." c={
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span className="iconbtn" style={{ width: 24, height: 24, border: '1px solid var(--field-border)', cursor: 'pointer' }} onClick={() => setN(Math.max(1, n - 1))}><Ic n="minus" s={10} /></span>
          <span style={{ fontFamily: 'var(--ide-mono)', minWidth: 18, textAlign: 'center' }}>{n}</span>
          <span className="iconbtn" style={{ width: 24, height: 24, border: '1px solid var(--field-border)', cursor: 'pointer' }} onClick={() => setN(Math.min(6, n + 1))}><Ic n="plus" s={10} /></span>
        </div>} />
      <Row t="Permissions by default" d="In chats and terminal agents." c={<div className="seg" style={{ fontSize: 12 }}><span>Ask</span><span className="on">Edits</span><span>Skip</span></div>} />
    </div>
  );
}

function Features() {
  return (
    <section className="sec" id="features" style={{ paddingTop: 80 }}>
      <div className="wrap">
        <Reveal><span className="eyebrow">Everything in reach</span></Reveal>
        <Reveal delay={0.05}><h2 className="h2">Built for how you <span className="dim">actually work.</span></h2></Reveal>
        <div className="bento">
          <Card className="span-4" title="Terminals that keep up" body="GPU-painted terminals on alacritty's engine: split panes, scrollback, pop-out windows. Every worktree opens with three, and the Run button knows your package manager.">
            <Scaled w={760} h={240}><Terminals h={240} /></Scaled>
          </Card>
          <Card className="span-2" title="Run anything" body="Run pnpm, cargo or make with one click. Save the commands you use every day.">
            <div style={{ display: 'grid', placeItems: 'center', height: '100%' }}><RunMenu /></div>
          </Card>
          <Card className="span-2" title="Right-click everything" body="Open files in a new tab or next to your agents, reveal, copy, rename, trash.">
            <div style={{ position: 'absolute', inset: '0 0 -28px', display: 'flex', justifyContent: 'center', maskImage: 'linear-gradient(#000 70%, transparent)' }}><FileMenu /></div>
          </Card>
          <Card className="span-4" title="A Project Brain for every repo" body="InsyDE reads main once (files, symbols, routes, merged PRs, decisions) and keeps a searchable graph. Agents get the relevant slice with their first message.">
            <BrainGraph />
          </Card>
          <Card className="span-3" title="Your agents, from your phone" body="Pair a phone by QR and keep working from the couch. Agents, diffs and terminals keep running on your Mac.">
            <Phone />
          </Card>
          <Card className="span-3" title="Quiet settings" body="Every option in one searchable place, saved as you change it. Esc takes you back.">
            <div style={{ display: 'flex', alignItems: 'center', height: '100%' }}><div style={{ width: '100%' }}><SettingsMini /></div></div>
          </Card>
        </div>
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ numbers */
function Count({ to, dec = 0, suffix }: { to: number; dec?: number; suffix?: string }) {
  const ref = useRef<HTMLElement>(null);
  const inView = useInView(ref, { once: true, amount: 0.6 });
  const mv = useMotionValue(0);
  const [v, setV] = useState('0');
  useEffect(() => {
    if (!inView) return;
    const c = animate(mv, to, { duration: 1.6, ease });
    const un = mv.on('change', (x) => setV(x.toFixed(dec)));
    return () => { c.stop(); un(); };
  }, [inView, to, dec, mv]);
  return <b ref={ref}>{v}{suffix && <small>{suffix}</small>}</b>;
}

function Numbers() {
  return (
    <section style={{ padding: '40px 0 120px' }}>
      <div className="wrap">
        <Reveal>
          <div className="numbers">
            <div className="num"><Count to={92} suffix="MB" /><span>Memory at idle with a worktree, a terminal and a restored chat.</span></div>
            <div className="num"><Count to={16} suffix="MB" /><span>The whole app. No bundled browser engine.</span></div>
            <div className="num"><Count to={0.1} dec={1} suffix="s" /><span>To create a task worktree, branch included.</span></div>
            <div className="num"><Count to={1.3} dec={1} suffix="s" /><span>To build a Project Brain of 1,313 nodes.</span></div>
          </div>
        </Reveal>
        <Reveal delay={0.1}><div className="faint" style={{ fontSize: 12, marginTop: 14 }}>Measured on an M-series Mac, release build.</div></Reveal>
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ native */
function Native() {
  const items = [
    { k: 'ui', h: 'Rust + GPUI', p: 'The UI framework behind Zed. Every pixel is drawn on the GPU; layout and paint stay on the main thread, everything else doesn’t.' },
    { k: 'agents', h: 'Agent Client Protocol', p: 'Agents are integrated over their protocols, never by parsing a terminal screen. Codex and Claude Code included.' },
    { k: 'terminal', h: 'alacritty_terminal', p: 'The emulator core of Alacritty, with xterm keys, bracketed paste and real scrollback.' },
    { k: 'data', h: 'Local SQLite', p: 'Projects, sessions and transcripts live on your Mac. Events are written before they are shown.' },
  ];
  return (
    <section className="sec" id="native" style={{ paddingTop: 40 }}>
      <div className="wrap">
        <Reveal><span className="eyebrow">Native to the core</span></Reveal>
        <Reveal delay={0.05}><h2 className="h2">Not a web page <span className="dim">in a trench coat.</span></h2></Reveal>
        <Reveal delay={0.1}><p className="sub">InsyDE is a macOS app written entirely in Rust. It opens fast, stays small, and doesn’t spin up a browser to show you a list.</p></Reveal>
        <div className="stack">
          {items.map((it, i) => (
            <Reveal key={it.h} delay={0.05 * i}><div className="stack-item" style={{ height: '100%' }}><div className="k">{it.k}</div><h5>{it.h}</h5><p>{it.p}</p></div></Reveal>
          ))}
        </div>
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ final */
function Final() {
  const ref = useRef<HTMLDivElement>(null);
  const { scrollYProgress } = useScroll({ target: ref, offset: ['start end', 'end end'] });
  const s = useSpring(useTransform(scrollYProgress, [0, 1], [0.92, 1]), { stiffness: 120, damping: 30 });
  return (
    <section className="final" ref={ref}>
      <div className="grid-bg" style={{ maskImage: 'radial-gradient(ellipse 60% 70% at 50% 60%, #000 30%, transparent 100%)' }} />
      <div className="glow" style={{ width: 700, height: 400, left: '50%', bottom: -120, transform: 'translateX(-50%)', background: 'radial-gradient(closest-side, rgba(217,119,87,.28), transparent)' }} />
      <motion.div className="wrap" style={{ scale: s, position: 'relative' }}>
        <div style={{ display: 'flex', justifyContent: 'center', gap: 10, marginBottom: 34 }}>
          {AGENTS.map((a, i) => (
            <motion.div key={a.key} initial={{ opacity: 0, y: 14 }} whileInView={{ opacity: 1, y: 0 }} viewport={{ once: true }} transition={{ delay: 0.06 * i, duration: 0.5, ease }}>
              <Mg k={a.key} s={38} />
            </motion.div>
          ))}
        </div>
        <h2 className="h2">Ship with a team<br /><span className="dim">of agents.</span></h2>
        <div className="ctas">
          <a className="btn btn-primary" href={DOWNLOAD} target="_blank" rel="noreferrer"><AppleMark />Download for macOS</a>
          <a className="btn btn-ghost" href={GITHUB} target="_blank" rel="noreferrer">Star on GitHub <span style={{ opacity: .6 }}>↗</span></a>
        </div>
      </motion.div>
    </section>
  );
}

function Footer() {
  return (
    <footer>
      <div className="wrap foot">
        <a href="#" className="logo"><img src="/icon.svg" alt="" style={{ width: 22, height: 22 }} />InsyDE <small>by Insyd</small></a>
        <span style={{ flex: 1 }} />
        <a href="#how">How it works</a><a href="#features">Features</a><a href={GITHUB} target="_blank" rel="noreferrer">GitHub</a>
        <span>Agent logos: LobeHub Icons (MIT)</span>
      </div>
    </footer>
  );
}

export default function App() {
  return (
    <>
      <Nav />
      <main>
        <Hero />
        <AgentStrip />
        <Story />
        <Features />
        <Numbers />
        <Native />
        <Final />
      </main>
      <Footer />
    </>
  );
}
