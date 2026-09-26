// InsyDE web: drive the agents, worktrees and terminals of an InsyDE
// machine from any browser. Projects and threads on the left, the chat in
// the middle, changes on the right, terminals in a drawer below.

import { Connection, takeTokenFromUrl, setToken, forgetToken } from '/rpc.js';
import { h, fill, icon, glyph, agentBadge, ago, fmtTokens, secs, basename, toast, menu, closeMenus, dialog, prompt, confirmBox } from '/ui.js';
import { render as md } from '/md.js';
import { TermDrawer } from '/term.js';

takeTokenFromUrl();
const conn = new Connection();

const ACCENTS = { violet: '#7C5CD6', green: '#2E9E6B', orange: '#E0733A', pink: '#D6457F' };
const POLICIES = [['Ask', 'Ask every time'], ['AcceptEdits', 'Auto-approve edits'], ['FullAccess', 'Full access']];
const mobile = () => innerWidth <= 820;

const S = {
  hello: null,
  projects: [],
  thread: null, // { id, title, agent, worktree, project, branch, items, meta }
  draft: { project: null, worktree: 'local', agent: null },
  filter: '',
  collapsed: new Set(JSON.parse(localStorage.getItem('insyde.collapsed') || '[]')),
  diffOpen: localStorage.getItem('insyde.diff') === '1',
  termOpen: localStorage.getItem('insyde.term') === '1',
  termH: +localStorage.getItem('insyde.termH') || 260,
  sideOpen: false,
  git: null,
  diffFile: null,
  useBrain: true,
  drafts: {},
};
// Phones start on the conversation; panels are opened on demand.
if (mobile()) { S.diffOpen = false; S.termOpen = false; }

// ---------- layout ----------

const el = {};
function layout() {
  el.menuBtn = h('button', { class: 'icon-btn only-mobile', title: 'Threads', onclick: () => setSide(!S.sideOpen) }, glyph('menu'));
  el.conn = h('span', { class: 'conn-dot' });
  el.host = h('span', { class: 'host-name' }, '…');
  el.topTitle = h('div', { class: 'top-title' });
  el.diffBtn = h('button', { class: 'tool-btn', title: 'Changes (⌘⇧G)', onclick: () => toggleDiff() }, glyph('diff'), h('span', { class: 'tool-label' }, 'Changes'), h('span', { class: 'tool-count' }));
  el.termBtn = h('button', { class: 'tool-btn', title: 'Terminal (⌘J)', onclick: () => toggleTerm() }, glyph('terminal'), h('span', { class: 'tool-label' }, 'Terminal'));
  el.top = h('header', { class: 'top' },
    el.menuBtn,
    h('div', { class: 'brand' }, h('span', { class: 'brand-name' }, 'IDE'), h('span', { class: 'brand-by' }, 'by Insyd')),
    h('button', { class: 'host-pill', title: 'Connection', onclick: (e) => hostMenu(e.currentTarget) }, el.conn, el.host, h('span', { class: 'web-tag' }, 'Web')),
    el.topTitle,
    h('div', { class: 'spacer' }),
    el.diffBtn, el.termBtn,
    h('button', { class: 'icon-btn', title: 'Search (⌘K)', onclick: () => palette() }, icon('search', 15)));

  el.side = h('aside', { class: 'side' });
  el.scrim = h('div', { class: 'scrim', onclick: () => { setSide(false); S.diffOpen && mobile() && toggleDiff(false); } });
  el.center = h('section', { class: 'center' });
  el.diff = h('aside', { class: 'diff' });
  el.drawerWrap = h('div', { class: 'drawer' });
  el.handle = h('div', { class: 'drawer-handle' });
  el.status = h('footer', { class: 'status' });
  el.work = h('div', { class: 'work' }, h('div', { class: 'work-row' }, el.center, el.diff), el.handle, el.drawerWrap);
  el.shell = h('div', { class: 'shell' }, el.top, h('div', { class: 'main' }, el.side, el.work), el.status, el.scrim);
  document.getElementById('app').replaceChildren(el.shell);

  term = new TermDrawer(conn, { onChange: () => {} });
  const drawerHead = h('div', { class: 'drawer-head' },
    h('span', { class: 'drawer-title' }, 'Terminals'),
    h('div', { class: 'spacer' }),
    el.runBtn = h('button', { class: 'small-btn', hidden: true }, icon('play', 12), h('span')),
    h('button', { class: 'icon-btn', title: 'Maximize', onclick: () => el.work.classList.toggle('term-max') }, glyph('expand', 14)),
    h('button', { class: 'icon-btn', title: 'Hide (⌘J)', onclick: () => toggleTerm(false) }, icon('close', 13)));
  el.drawerWrap.append(drawerHead, term.el);
  dragHandle();
  applyPanels();
}
let term;

function dragHandle() {
  el.handle.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    const start = e.clientY, h0 = S.termH;
    const move = (ev) => {
      S.termH = Math.max(120, Math.min(innerHeight - 180, h0 + (start - ev.clientY)));
      el.work.style.setProperty('--term-h', `${S.termH}px`);
    };
    const up = () => {
      removeEventListener('pointermove', move);
      removeEventListener('pointerup', up);
      localStorage.setItem('insyde.termH', S.termH);
      term.fitActive();
    };
    addEventListener('pointermove', move);
    addEventListener('pointerup', up);
  });
}

function applyPanels() {
  el.work.classList.toggle('with-diff', S.diffOpen);
  el.work.classList.toggle('with-term', S.termOpen);
  el.work.style.setProperty('--term-h', `${S.termH}px`);
  el.diffBtn.classList.toggle('on', S.diffOpen);
  el.termBtn.classList.toggle('on', S.termOpen);
  el.shell.classList.toggle('side-open', S.sideOpen);
  el.shell.classList.toggle('overlay', mobile() && (S.sideOpen || S.diffOpen || S.termOpen));
}

function setSide(open) {
  S.sideOpen = open;
  // On a phone every panel is a sheet: only one is open at a time.
  if (open && mobile()) { S.diffOpen = false; S.termOpen = false; }
  applyPanels();
}

function currentWorktree() {
  if (S.thread) return S.thread.worktree;
  const p = S.projects.find((x) => x.path === S.draft.project);
  if (!p) return null;
  if (S.draft.worktree && S.draft.worktree !== 'local' && S.draft.worktree !== 'new') return S.draft.worktree;
  return (p.worktrees.find((w) => w.primary) || p.worktrees[0])?.path || p.path;
}

function toggleDiff(force) {
  S.diffOpen = force ?? !S.diffOpen;
  localStorage.setItem('insyde.diff', S.diffOpen ? '1' : '0');
  if (S.diffOpen && mobile()) { S.termOpen = false; S.sideOpen = false; }
  applyPanels();
  if (S.diffOpen) loadGit();
}

function toggleTerm(force) {
  S.termOpen = force ?? !S.termOpen;
  localStorage.setItem('insyde.term', S.termOpen ? '1' : '0');
  if (S.termOpen && mobile()) { S.diffOpen = false; S.sideOpen = false; }
  applyPanels();
  const wt = currentWorktree();
  if (S.termOpen && wt) { term.load(wt); updateRunBtn(wt); }
  else if (!S.termOpen) document.activeElement?.blur();
}

/** "Run" in the drawer header: the project's dev command (cargo run, pnpm dev…). */
async function updateRunBtn(wt) {
  const s = await conn.call('script', { worktree: wt }).catch(() => null);
  el.runBtn.hidden = !s;
  if (!s) return;
  el.runBtn.title = s.command;
  fill(el.runBtn.lastChild, s.label);
  el.runBtn.onclick = () => term.open(s.command);
}

// ---------- theme ----------

function applyTheme() {
  const t = S.hello?.theme || 'dark';
  const dark = t === 'dark' || (t === 'system' && matchMedia('(prefers-color-scheme: dark)').matches);
  document.documentElement.dataset.theme = dark ? 'dark' : 'light';
  const a = ACCENTS[S.hello?.accent];
  const root = document.documentElement.style;
  if (a) root.setProperty('--accent', a); else root.removeProperty('--accent');
  document.documentElement.classList.toggle('custom-accent', !!a);
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', dark ? '#1D1D1C' : '#FFFFFF');
  term?.refreshTheme();
}
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', applyTheme);

// ---------- connection ----------

function renderConn() {
  el.conn.className = `conn-dot ${conn.state}`;
  el.conn.title = { open: 'Connected', connecting: 'Connecting…', offline: 'Reconnecting…', unpaired: 'Not paired' }[conn.state];
  el.shell?.classList.toggle('is-offline', conn.state === 'offline');
}

conn.on('state', (s) => {
  if (s === 'unpaired') { pairing(); return; }
  if (!el.shell) layout();
  renderConn();
});

conn.on('open', async () => {
  if (!el.shell || !el.shell.isConnected) layout();
  renderConn();
  try {
    S.hello = await conn.call('hello');
    el.host.textContent = S.hello.host;
    document.title = `IDE by Insyd · ${S.hello.host}`;
    S.useBrain = S.hello.brain_by_default;
    applyTheme();
    await loadProjects();
    route();
    if (S.termOpen) await term.reattach();
  } catch (e) { toast(e.message, 'error'); }
});

conn.on('projects', () => loadProjects());
conn.on('status', (e) => {
  for (const p of S.projects) for (const w of p.worktrees) for (const t of w.threads) if (t.id === e.id) t.status = e.status;
  renderSide();
});
conn.on('thread', (e) => {
  if (!S.thread || S.thread.id !== e.id) return;
  applyDiff(e);
});

// ---------- pairing ----------

function pairing() {
  el.shell = null;
  const input = h('input', { class: 'field', placeholder: 'Paste the pairing link or code', autofocus: true, spellcheck: 'false' });
  const go = () => {
    const v = input.value.trim();
    const m = v.match(/t=([0-9a-f]{32,128})/i) || v.match(/^([0-9a-f]{32,128})$/i);
    if (!m) { toast('That is not a pairing link.', 'error'); return; }
    setToken(m[1]);
    conn.connect();
  };
  input.addEventListener('keydown', (e) => { if (e.key === 'Enter') go(); });
  document.getElementById('app').replaceChildren(h('div', { class: 'pair' },
    h('div', { class: 'pair-card' },
      h('img', { src: '/icon.svg', class: 'pair-logo', alt: '' }),
      h('h1', null, 'Connect to IDE by Insyd'),
      h('p', null, 'On the computer running IDE by Insyd, open Settings › Web access and scan the QR code, or copy the link and paste it here.'),
      input,
      h('button', { class: 'btn primary wide', onclick: go }, 'Connect'),
      h('p', { class: 'pair-note' }, 'The link works like a key to that computer. Only open it on devices you trust.'))));
  setTimeout(() => input.focus(), 50);
}

// ---------- routing ----------

function route() {
  const m = location.hash.match(/^#\/t\/(\d+)/);
  if (m) { openThread(+m[1]); return; }
  const n = location.hash.match(/^#\/new(?:\/(.*))?/);
  newThread(n?.[1] ? decodeURIComponent(n[1]) : null, false);
}
addEventListener('hashchange', () => { if (conn.state === 'open') route(); });

// ---------- projects & sidebar ----------

async function loadProjects(refresh = false) {
  try {
    S.projects = await conn.call('projects', { refresh });
  } catch { return; }
  renderSide();
  if (!S.thread) renderCenter();
}

function allThreads() {
  const out = [];
  for (const p of S.projects) for (const w of p.worktrees) for (const t of w.threads) out.push({ ...t, project: p, worktree: w });
  return out.sort((a, b) => b.updated - a.updated);
}

/** Build the sidebar frame once; later updates only redraw the list. */
function renderSide() {
  if (!el.side) return;
  if (el.sideList?.isConnected) { renderSideList(); return; }
  const search = h('div', { class: 'side-search' }, icon('search', 13),
    h('input', { placeholder: 'Search threads', value: S.filter, oninput: (e) => { S.filter = e.target.value; renderSideList(); } }));
  const newBtn = h('button', { class: 'new-thread', onclick: () => newThread(S.thread?.projectPath || S.draft.project) }, icon('plus', 14), 'New thread', h('kbd', null, '⌘⇧O'));
  el.sideList = h('div', { class: 'side-list' });
  fill(el.side, h('div', { class: 'side-top' }, newBtn, search), el.sideList,
    h('div', { class: 'side-foot' },
      h('button', { class: 'side-link', onclick: addProject }, icon('folder', 14), 'Add project'),
      h('button', { class: 'icon-btn', title: 'Refresh', onclick: () => loadProjects(true) }, icon('refresh', 14))));
  renderSideList();
}

function renderSideList() {
  const q = S.filter.trim().toLowerCase();
  const list = el.sideList;
  fill(list);
  if (!S.projects.length) {
    list.append(h('div', { class: 'side-empty' }, 'No projects yet.', h('button', { class: 'btn small', onclick: addProject }, 'Add a project')));
    return;
  }
  for (const p of S.projects) {
    const threads = [];
    for (const w of p.worktrees) for (const t of w.threads) threads.push({ t, w });
    threads.sort((a, b) => b.t.updated - a.t.updated);
    const shown = q ? threads.filter(({ t }) => t.title.toLowerCase().includes(q)) : threads;
    if (q && !shown.length) continue;
    const collapsed = S.collapsed.has(p.path) && !q;
    const head = h('div', { class: 'proj', onclick: () => { collapsed ? S.collapsed.delete(p.path) : S.collapsed.add(p.path); localStorage.setItem('insyde.collapsed', JSON.stringify([...S.collapsed])); renderSideList(); } },
      icon(collapsed ? 'chevron-right' : 'chevron-down', 12, 'proj-chev'),
      h('span', { class: 'proj-letter' }, (p.name.match(/[a-z0-9]/i)?.[0] || '?').toUpperCase()),
      h('span', { class: 'proj-name' }, p.name),
      p.remote ? h('span', { class: 'chip' }, 'SSH') : null,
      h('button', { class: 'icon-btn ghost', title: `New thread in ${p.name}`, onclick: (e) => { e.stopPropagation(); newThread(p.path); } }, icon('plus', 13)));
    list.append(head);
    if (collapsed) continue;
    if (!shown.length) list.append(h('div', { class: 'thread-empty' }, 'No threads'));
    for (const { t, w } of shown.slice(0, q ? 50 : 30)) {
      const on = S.thread?.id === t.id;
      const st = t.status || {};
      const dot = st.attention ? h('span', { class: 'st attention', title: 'Needs your approval' })
        : st.running ? h('span', { class: 'st running', title: 'Working' })
        : st.error ? h('span', { class: 'st error', title: 'Stopped with an error' }) : null;
      const row = h('a', { class: `thread${on ? ' on' : ''}`, href: `#/t/${t.id}`, onclick: () => mobile() && setSide(false) },
        agentBadge(t.agent, 18),
        h('div', { class: 'thread-text' },
          h('div', { class: 'thread-title' }, t.title),
          h('div', { class: 'thread-meta' }, w.primary ? null : h('span', { class: 'thread-branch' }, icon('branch', 11), w.branch), h('span', null, ago(t.updated)))),
        dot,
        h('button', { class: 'icon-btn ghost thread-more', title: 'More', onclick: (e) => { e.preventDefault(); e.stopPropagation(); threadMenu(e.currentTarget, t); } }, glyph('dots', 14)));
      row.addEventListener('contextmenu', (e) => { e.preventDefault(); threadMenu(row, t); });
      list.append(row);
    }
  }
}

function threadMenu(anchor, t) {
  menu(anchor, [
    { label: 'Rename…', run: async () => {
      const title = await prompt('Rename thread', { value: t.title });
      if (title?.trim()) {
        await conn.call('thread.rename', { id: t.id, title }).catch((e) => toast(e.message, 'error'));
        if (S.thread?.id === t.id) { S.thread.title = title.trim(); renderHead(); }
      }
    } },
    { label: 'Copy link', run: () => navigator.clipboard?.writeText(`${location.origin}/#/t/${t.id}`) },
    { sep: true },
    { label: 'Archive', danger: true, run: async () => {
      if (!(await confirmBox('Archive this thread?', 'It leaves the sidebar. The agent stops if it is still working.', 'Archive'))) return;
      await conn.call('thread.archive', { id: t.id }).catch((e) => toast(e.message, 'error'));
      if (S.thread?.id === t.id) location.hash = '#/new';
    } },
  ], { align: 'right' });
}

async function addProject() {
  let dir = null;
  const picked = await dialog((box, close) => {
    const list = h('div', { class: 'picker-list' });
    const pathLine = h('div', { class: 'picker-path' });
    const manual = h('input', { class: 'field', placeholder: 'Or type a path, e.g. ~/code/app or host:/srv/repo' });
    manual.addEventListener('keydown', (e) => { if (e.key === 'Enter' && manual.value.trim()) close(manual.value.trim()); });
    async function go(path) {
      try {
        dir = await conn.call('fs.list', { path });
      } catch (e) { toast(e.message, 'error'); return; }
      fill(pathLine, h('button', { class: 'icon-btn', title: 'Up', disabled: !dir.parent, onclick: () => go(dir.parent) }, icon('chevron-left', 13)), h('span', null, dir.path));
      fill(list, ...dir.dirs.map((d) => h('button', { class: `picker-row${d.repo ? ' repo' : ''}`, ondblclick: () => d.repo ? close(d.path) : go(d.path), onclick: () => d.repo ? close(d.path) : go(d.path) },
        icon(d.repo ? 'branch' : 'folder', 14), h('span', null, d.name), d.repo ? h('span', { class: 'chip' }, 'git') : null)));
      if (!dir.dirs.length) list.append(h('div', { class: 'picker-empty' }, 'No folders here.'));
    }
    box.append(h('div', { class: 'dialog-title' }, `Add a project on ${S.hello?.host || 'this machine'}`),
      h('p', { class: 'dialog-body' }, 'Pick a git repository. Folders marked git open directly.'),
      pathLine, list, manual,
      h('div', { class: 'dialog-actions' },
        h('button', { class: 'btn', onclick: () => close(null) }, 'Cancel'),
        h('button', { class: 'btn primary', onclick: () => close(manual.value.trim() || dir?.path) }, 'Add this folder')));
    go(null);
  }, { wide: true });
  if (!picked) return;
  const path = picked.startsWith('~/') && S.hello?.home ? S.hello.home + picked.slice(1) : picked;
  try {
    const r = await conn.call('project.add', { path });
    await loadProjects(true);
    newThread(r.path);
    toast('Project added');
  } catch (e) { toast(e.message, 'error'); }
}

// ---------- center ----------

function renderCenter() {
  if (S.thread) renderThread(); else renderNew();
}

function newThread(project, push = true) {
  if (S.thread) conn.send('thread.leave', { id: S.thread.id });
  S.thread = null;
  S.git = null;
  S.draft.project = project || S.draft.project || S.projects[0]?.path || null;
  S.draft.agent = S.draft.agent || S.hello?.default_agent || 'claude';
  if (push && !location.hash.startsWith('#/new')) history.pushState(null, '', '#/new');
  renderSide();
  renderCenter();
  renderStatus();
  if (S.diffOpen) loadGit();
  const wt = currentWorktree();
  if (S.termOpen && wt) { term.load(wt); updateRunBtn(wt); }
  if (mobile()) setSide(false);
}

function renderNew() {
  fill(el.topTitle);
  const p = S.projects.find((x) => x.path === S.draft.project);
  if (!S.projects.length) {
    fill(el.center, h('div', { class: 'hero' },
      h('img', { src: '/icon.svg', class: 'hero-logo', alt: '' }),
      h('h1', null, 'Welcome to IDE by Insyd on the web'),
      h('p', null, `Add a repository on ${S.hello?.host || 'your machine'} to start a thread with an agent.`),
      h('button', { class: 'btn primary', onclick: addProject }, icon('folder', 14), 'Add project')));
    return;
  }
  const agents = (S.hello?.agents || []);
  const agent = agents.find((a) => a.key === S.draft.agent) || agents[0];
  const primary = p?.worktrees.find((w) => w.primary) || p?.worktrees[0];
  const envLabel = S.draft.worktree === 'new' ? 'New worktree'
    : S.draft.worktree === 'local' ? `Local · ${primary?.branch || 'main'}`
    : `Worktree · ${p?.worktrees.find((w) => w.path === S.draft.worktree)?.branch || basename(S.draft.worktree)}`;
  const projBtn = h('button', { class: 'pick' }, h('span', { class: 'proj-letter sm' }, (p?.name.match(/[a-z0-9]/i)?.[0] || '?').toUpperCase()), p?.name || 'Project', icon('chevron-down', 11));
  projBtn.onclick = () => menu(projBtn, S.projects.map((x) => ({ label: x.name, hint: x.stack, checked: x.path === S.draft.project, run: () => { S.draft.project = x.path; S.draft.worktree = 'local'; renderNew(); if (S.termOpen) term.load(currentWorktree()); } })));
  const envBtn = h('button', { class: 'pick' }, icon(S.draft.worktree === 'new' ? 'plus' : 'branch', 13), envLabel, icon('chevron-down', 11));
  envBtn.onclick = () => menu(envBtn, [
    { header: 'Where the agent works' },
    { label: `Local · ${primary?.branch || 'main'}`, hint: 'main checkout', checked: S.draft.worktree === 'local', run: () => { S.draft.worktree = 'local'; renderNew(); } },
    { label: 'New worktree', hint: 'own branch', checked: S.draft.worktree === 'new', run: () => { S.draft.worktree = 'new'; renderNew(); } },
    ...(p?.worktrees.filter((w) => !w.primary).length ? [{ sep: true }, { header: 'Existing worktrees' }] : []),
    ...(p?.worktrees.filter((w) => !w.primary).map((w) => ({ label: w.branch, hint: `+${w.added} −${w.removed}`, checked: S.draft.worktree === w.path, run: () => { S.draft.worktree = w.path; renderNew(); } })) || []),
  ], { width: 280 });
  const agentBtn = h('button', { class: 'pick' }, agentBadge(agent?.key || 'claude', 16), agent?.name || 'Agent', icon('chevron-down', 11));
  agentBtn.onclick = () => menu(agentBtn, agents.map((a) => ({ label: a.name, icon: agentBadge(a.key, 18), hint: a.available ? (a.key === S.hello.default_agent ? 'default' : '') : 'not installed', disabled: !a.available, checked: a.key === S.draft.agent, run: () => { S.draft.agent = a.key; renderNew(); } })), { width: 240 });

  const composer = buildComposer({
    placeholder: `Ask ${agent?.name || 'the agent'} to build, fix or explain something in ${p?.name || 'this project'}…`,
    draftKey: `new:${S.draft.project}`,
    onSend: startThread,
    tools: [agentBtn],
  });
  fill(el.center, h('div', { class: 'new-wrap' },
    h('div', { class: 'new-hero' },
      h('h1', null, 'What should we work on?'),
      h('div', { class: 'new-picks' }, projBtn, envBtn)),
    composer.el,
    h('div', { class: 'new-hints' },
      hint('⌘K', 'search threads'), hint('⌘J', 'terminal'), hint('⌘⇧G', 'changes'))));
  setTimeout(() => !mobile() && composer.focus(), 0);
}

function hint(k, t) { return h('span', { class: 'hint' }, h('kbd', null, k), t); }

async function startThread(text) {
  const p = S.projects.find((x) => x.path === S.draft.project);
  if (!p) return false;
  const params = { project: p.path, agent: S.draft.agent, title: text.split('\n').find((l) => l.trim())?.slice(0, 60) || '' };
  if (S.draft.worktree === 'new') params.worktree = 'new';
  else if (S.draft.worktree !== 'local') params.worktree = S.draft.worktree;
  else params.worktree = (p.worktrees.find((w) => w.primary) || p.worktrees[0])?.path || p.path;
  try {
    if (params.worktree === 'new') toast('Creating a worktree…');
    const r = await conn.call('thread.new', params);
    history.pushState(null, '', `#/t/${r.id}`);
    await openThread(r.id);
    await conn.call('thread.send', { id: r.id, text, brain: S.useBrain });
    S.draft.worktree = 'local';
    return true;
  } catch (e) { toast(e.message, 'error'); return false; }
}

// ---------- thread ----------

async function openThread(id) {
  if (S.thread && S.thread.id !== id) conn.send('thread.leave', { id: S.thread.id });
  let r;
  try { r = await conn.call('thread.open', { id }); } catch (e) { toast(e.message, 'error'); location.hash = '#/new'; return; }
  const proj = S.projects.find((p) => p.worktrees.some((w) => w.path === r.worktree));
  S.thread = { ...r, projectPath: proj?.path };
  S.draft.project = proj?.path || S.draft.project;
  S.git = null;
  renderSide();
  renderThread();
  renderStatus();
  if (S.diffOpen) loadGit();
  if (S.termOpen) { term.load(r.worktree); updateRunBtn(r.worktree); }
}

function renderHead() {
  const t = S.thread;
  if (!t) return;
  fill(el.topTitle, 
    agentBadge(t.agent, 18),
    h('span', { class: 'top-thread', title: 'Rename', onclick: async () => {
      const title = await prompt('Rename thread', { value: t.title });
      if (title?.trim()) { await conn.call('thread.rename', { id: t.id, title }); t.title = title.trim(); renderHead(); }
    } }, t.title),
    t.branch ? h('span', { class: 'chip branch' }, icon('branch', 11), t.branch) : null);
}

function renderThread() {
  const t = S.thread;
  renderHead();
  el.timeline = h('div', { class: 'timeline' });
  el.tlInner = h('div', { class: 'tl-inner' });
  el.timeline.append(el.tlInner);
  el.timeline.addEventListener('click', onTimelineClick);
  t.nodes = t.items.map((it, i) => itemNode(it, i));
  el.tlInner.append(...t.nodes);
  if (!t.items.length) el.tlInner.append(h('div', { class: 'tl-empty' }, 'Say what you need. The agent works in ', h('code', null, basename(t.worktree)), '.'));
  el.working = h('div', { class: 'working' });
  el.perm = h('div', { class: 'perm-slot' });
  const composer = buildComposer({
    placeholder: 'Ask for changes, send follow-ups, or paste an error…',
    draftKey: `t:${t.id}`,
    onSend: async (text) => {
      try { await conn.call('thread.send', { id: t.id, text, brain: S.useBrain }); return true; }
      catch (e) { toast(e.message, 'error'); return false; }
    },
    thread: t,
  });
  el.composer = composer;
  fill(el.center, el.timeline, h('div', { class: 'dock' }, el.working, el.perm, composer.el));
  renderMeta();
  requestAnimationFrame(() => { el.timeline.scrollTop = el.timeline.scrollHeight; });
  if (!mobile()) setTimeout(() => composer.focus(), 0);
}

function applyDiff(e) {
  const t = S.thread;
  const tl = el.timeline;
  const stick = tl && tl.scrollHeight - tl.scrollTop - tl.clientHeight < 80;
  if (!t.items.length && e.len) el.tlInner.querySelector('.tl-empty')?.remove();
  for (const [i, item] of e.set) {
    t.items[i] = item;
    const node = itemNode(item, i);
    if (t.nodes[i]) { t.nodes[i].replaceWith(node); } else { el.tlInner.append(node); }
    t.nodes[i] = node;
  }
  while (t.nodes.length > e.len) t.nodes.pop().remove();
  t.items.length = e.len;
  const wasRunning = t.meta.running;
  t.meta = e.meta;
  renderMeta();
  if (stick) tl.scrollTop = tl.scrollHeight;
  if (wasRunning && !t.meta.running) {
    if (S.diffOpen) loadGit();
    if (document.hidden && 'Notification' in window && Notification.permission === 'granted') {
      new Notification(t.title, { body: 'The agent finished.', icon: '/icon.svg' });
    }
  }
}

let tick;
function renderMeta() {
  const t = S.thread;
  if (!t) return;
  const m = t.meta || {};
  clearInterval(tick);
  fill(el.working);
  if (m.running) {
    const label = h('span', { class: 'working-time' });
    const upd = () => { label.textContent = m.turn_started ? secs(Date.now() / 1000 - m.turn_started) : ''; };
    upd();
    tick = setInterval(upd, 1000);
    el.working.append(h('span', { class: 'spinner' }), h('span', null, m.status_line || (m.ready ? 'Working' : `Starting ${agentName(t.agent)}`)), label,
      h('button', { class: 'small-btn', onclick: () => conn.send('thread.cancel', { id: t.id }) }, icon('stop', 11), 'Stop'));
  } else if (m.error) {
    el.working.append(h('div', { class: 'err-line' }, m.error));
  }
  el.working.classList.toggle('on', el.working.childElementCount > 0);
  fill(el.perm);
  if (m.permission) {
    const p = m.permission;
    el.perm.append(h('div', { class: 'perm' },
      h('div', { class: 'perm-head' }, h('span', { class: 'st attention' }), h('b', null, p.title || 'Permission needed')),
      p.detail ? h('pre', { class: 'perm-detail' }, p.detail) : null,
      h('div', { class: 'perm-actions' },
        ...p.options.map(([id, label, allow]) => h('button', { class: `btn ${allow ? 'primary' : ''}`, onclick: () => conn.call('thread.answer', { id: t.id, option: id }) }, label)),
        h('button', { class: 'btn ghost', onclick: () => conn.call('thread.answer', { id: t.id, option: null }) }, 'Stop'))));
  }
  el.composer?.update(m);
}

function agentName(key) {
  return S.hello?.agents.find((a) => a.key === key)?.name || key;
}

function onTimelineClick(e) {
  const copy = e.target.closest('[data-copy]');
  if (copy) {
    const code = copy.parentElement.querySelector('code')?.textContent || '';
    navigator.clipboard?.writeText(code).then(() => { copy.textContent = 'Copied'; setTimeout(() => (copy.textContent = 'Copy'), 1200); });
    return;
  }
  const fold = e.target.closest('[data-fold]');
  if (fold) fold.parentElement.classList.toggle('open');
}

const KIND_ICON = { Read: 'file', Edit: 'file', Delete: 'trash', Move: 'file', Search: 'search', Bash: 'play', Think: 'brain', Fetch: 'popout', Other: 'running' };

function itemNode(item, i) {
  const [kind, v] = Object.entries(item)[0] || [];
  switch (kind) {
    case 'User':
      return h('div', { class: 'msg user', 'data-i': i }, h('div', { class: 'bubble' }, v.text));
    case 'Agent':
      return h('div', { class: 'msg agent md', 'data-i': i, html: md(v.text) });
    case 'Thought':
      return h('div', { class: 'msg thought', 'data-i': i },
        h('button', { class: 'fold', 'data-fold': '' }, icon('chevron-right', 11), 'Thinking'),
        h('div', { class: 'thought-body md', html: md(v.text) }));
    case 'Tools': {
      const calls = v.calls || [];
      const open = calls.some((c) => c.status === 'Running' || c.status === 'Pending') || calls.length <= 6;
      const add = calls.reduce((a, c) => a + (c.added || 0), 0), rem = calls.reduce((a, c) => a + (c.removed || 0), 0);
      return h('div', { class: `msg tools${open ? ' open' : ''}`, 'data-i': i },
        h('button', { class: 'fold tools-head', 'data-fold': '' }, icon('chevron-right', 11), `${calls.length} ${calls.length === 1 ? 'action' : 'actions'}`,
          add || rem ? h('span', { class: 'stat' }, h('span', { class: 'add' }, `+${add}`), h('span', { class: 'rem' }, `−${rem}`)) : null),
        h('div', { class: 'tools-body' }, ...calls.map(toolRow)));
    }
    case 'Plan':
      return h('div', { class: 'msg plan', 'data-i': i }, h('div', { class: 'plan-head' }, 'Plan'),
        ...(v.entries || []).map((e) => h('div', { class: `plan-row${e.done ? ' done' : ''}${e.active ? ' active' : ''}` }, h('span', { class: 'plan-box' }, e.done ? icon('check', 10) : null), e.text)));
    case 'Worked':
      return h('div', { class: 'msg worked', 'data-i': i }, h('span', null, `Worked for ${secs(v.secs)}`));
    case 'Notice':
      return h('div', { class: `msg notice${v.error ? ' error' : ''}`, 'data-i': i }, v.text);
    default:
      return h('div', { 'data-i': i });
  }
}

function toolRow(c) {
  const st = { Pending: 'pending', Running: 'running', Done: 'done', Failed: 'failed' }[c.status] || '';
  return h('div', { class: `tool ${st}` },
    h('span', { class: 'tool-ic' }, c.status === 'Running' ? h('span', { class: 'spinner sm' }) : c.status === 'Failed' ? icon('cross', 12) : icon(KIND_ICON[c.kind] || 'running', 12)),
    h('span', { class: 'tool-title' }, c.title || c.kind),
    c.arg ? h('code', { class: 'tool-arg', title: c.arg }, c.arg) : null,
    h('span', { class: 'tool-meta' }, c.added || c.removed ? h('span', { class: 'stat' }, h('span', { class: 'add' }, `+${c.added}`), h('span', { class: 'rem' }, `−${c.removed}`)) : c.meta || ''));
}

// ---------- composer ----------

function buildComposer({ placeholder, draftKey, onSend, tools = [], thread = null }) {
  const ta = h('textarea', { class: 'composer-input', rows: 1, placeholder, spellcheck: 'true' });
  ta.value = S.drafts[draftKey] || '';
  const grow = () => { ta.style.height = 'auto'; ta.style.height = `${Math.min(ta.scrollHeight, innerHeight * 0.4)}px`; };
  const sendBtn = h('button', { class: 'send', title: 'Send (Enter)' }, icon('send', 14));
  let busy = false;
  const send = async () => {
    const text = ta.value.trim();
    if (!text || busy) return;
    if (thread?.meta?.running) { conn.send('thread.cancel', { id: thread.id }); return; }
    busy = true;
    ta.value = '';
    S.drafts[draftKey] = '';
    grow();
    const ok = await onSend(text);
    busy = false;
    if (!ok) { ta.value = text; grow(); }
  };
  ta.addEventListener('input', () => { S.drafts[draftKey] = ta.value; grow(); });
  ta.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing && !(mobile() && matchMedia('(pointer: coarse)').matches)) { e.preventDefault(); send(); }
  });
  sendBtn.addEventListener('click', () => {
    if (thread?.meta?.running && !ta.value.trim()) { conn.send('thread.cancel', { id: thread.id }); return; }
    send();
  });
  const bar = h('div', { class: 'composer-bar' });
  const right = h('div', { class: 'composer-right' });
  const box = h('div', { class: 'composer' }, ta, h('div', { class: 'composer-foot' }, bar, right));

  function update(m = {}) {
    fill(bar, ...tools);
    if (thread) {
      bar.append(h('span', { class: 'pick static' }, agentBadge(thread.agent, 16), agentName(thread.agent)));
      if (m.models?.length) {
        const cur = m.models.find(([id]) => id === m.model);
        const b = h('button', { class: 'pick' }, cur?.[1] || 'Model', icon('chevron-down', 11));
        b.onclick = () => menu(b, m.models.map(([id, name]) => ({ label: name, checked: id === m.model, run: () => conn.call('thread.model', { id: thread.id, value: id }) })), { width: 260 });
        bar.append(b);
      }
      if (m.modes?.length > 1) {
        const cur = m.modes.find(([id]) => id === m.mode);
        const b = h('button', { class: 'pick' }, cur?.[1] || 'Mode', icon('chevron-down', 11));
        b.onclick = () => menu(b, m.modes.map(([id, name]) => ({ label: name, checked: id === m.mode, run: () => conn.call('thread.mode', { id: thread.id, value: id }) })));
        bar.append(b);
      }
      const pol = POLICIES.find(([k]) => k === m.policy) || POLICIES[1];
      const pb = h('button', { class: `pick${m.policy === 'FullAccess' ? ' warn' : ''}` }, pol[1], icon('chevron-down', 11));
      pb.onclick = () => menu(pb, POLICIES.map(([k, label]) => ({ label, checked: k === m.policy, run: () => conn.call('thread.policy', { id: thread.id, value: k }) })));
      bar.append(pb);
    }
    if (!thread || !thread.items.length) {
      const chip = h('button', { class: `chip-btn${S.useBrain ? ' on' : ''}`, title: 'Attach the Project Brain digest to the first message' }, icon('brain', 12), 'Brain context');
      chip.onclick = () => { S.useBrain = !S.useBrain; update(m); };
      bar.append(chip);
    }
    fill(right);
    if (thread && m.size) {
      const pct = Math.min(100, Math.round((m.used / m.size) * 100));
      right.append(h('span', { class: `ctx${pct > 85 ? ' hot' : ''}`, title: `${fmtTokens(m.used)} of ${fmtTokens(m.size)} tokens${m.cost ? ` · $${m.cost.toFixed(2)}` : ''}` },
        h('span', { class: 'ctx-bar' }, h('span', { style: { width: `${pct}%` } })), `${pct}%`));
    }
    fill(sendBtn, icon(thread?.meta?.running ? 'stop' : 'send', 14));
    sendBtn.classList.toggle('stop', !!thread?.meta?.running);
    sendBtn.title = thread?.meta?.running ? 'Stop' : 'Send (Enter)';
    right.append(sendBtn);
  }
  update(thread?.meta || {});
  setTimeout(grow, 0);
  return { el: box, update, focus: () => ta.focus() };
}

// ---------- changes & git ----------

async function loadGit() {
  const wt = currentWorktree();
  if (!wt) { fill(el.diff); return; }
  el.diff.classList.add('loading');
  try { S.git = await conn.call('git.status', { worktree: wt }); S.git.worktree = wt; } catch (e) { S.git = { error: e.message, files: [] }; }
  el.diff.classList.remove('loading');
  renderDiff();
  renderStatus();
}

function renderDiff() {
  const g = S.git;
  const count = el.diffBtn.querySelector('.tool-count');
  count.textContent = g?.files?.length ? String(g.files.length) : '';
  if (!g) { fill(el.diff); return; }
  const head = h('div', { class: 'panel-head' },
    h('span', { class: 'panel-title' }, 'Changes'),
    g.files.length ? h('span', { class: 'stat' }, h('span', { class: 'add' }, `+${g.added}`), h('span', { class: 'rem' }, `−${g.removed}`)) : null,
    h('div', { class: 'spacer' }),
    h('button', { class: 'icon-btn', title: 'Refresh', onclick: loadGit }, icon('refresh', 13)),
    h('button', { class: 'icon-btn', title: 'Close', onclick: () => toggleDiff(false) }, icon('close', 13)));
  const msg = h('textarea', { class: 'field commit-msg', rows: 2, placeholder: `Commit message for ${g.branch || 'this branch'}` });
  const git = h('div', { class: 'git-box' },
    h('div', { class: 'git-row' }, icon('branch', 12), h('b', null, g.branch || 'detached'), h('span', { class: 'muted' }, `from ${g.base}`),
      h('div', { class: 'spacer' }), g.ahead || g.behind ? h('span', { class: 'muted' }, `↑${g.ahead} ↓${g.behind}`) : null),
    g.dirty ? msg : null,
    h('div', { class: 'git-actions' },
      g.dirty ? h('button', { class: 'btn small', onclick: () => gitAct('git.commit', { message: msg.value }, 'Committed') }, glyph('commit', 13), 'Commit all') : null,
      h('button', { class: 'btn small', onclick: () => gitAct('git.push', {}, 'Pushed') }, glyph('upload', 13), 'Push'),
      h('button', { class: 'btn small primary', onclick: createPr }, icon('pr', 13), 'Create PR')));
  const files = h('div', { class: 'files' });
  if (g.error) files.append(h('div', { class: 'panel-empty' }, g.error));
  else if (!g.files.length) files.append(h('div', { class: 'panel-empty' }, 'No changes against ', h('code', null, g.base), '.'));
  for (const f of g.files) {
    const open = S.diffFile === f.path;
    const row = h('button', { class: `file${open ? ' open' : ''}`, onclick: () => { S.diffFile = open ? null : f.path; renderDiff(); } },
      icon(open ? 'chevron-down' : 'chevron-right', 11),
      h('span', { class: 'file-name' }, basename(f.path)),
      h('span', { class: 'file-dir' }, f.path.includes('/') ? f.path.slice(0, f.path.lastIndexOf('/')) : ''),
      h('span', { class: 'stat' }, f.binary ? 'bin' : [h('span', { class: 'add' }, `+${f.added}`), h('span', { class: 'rem' }, `−${f.removed}`)]));
    files.append(row);
    if (open) {
      const box = h('div', { class: 'patch' }, h('div', { class: 'panel-empty' }, 'Loading…'));
      files.append(box);
      conn.call('git.diff', { worktree: g.worktree, path: f.path }).then((r) => fill(box, patchView(r.lines))).catch((e) => fill(box, h('div', { class: 'panel-empty' }, e.message)));
    }
  }
  fill(el.diff, head, git, files);
}

function patchView(lines) {
  const t = h('div', { class: 'patch-lines' });
  let shown = 0;
  for (const [text, o, n] of lines) {
    if (/^(diff --git|index |--- |\+\+\+ |new file|deleted file|similarity|rename )/.test(text)) continue;
    if (++shown > 3000) { t.append(h('div', { class: 'pl hunk' }, h('span'), h('span'), h('span', null, 'Diff truncated'))); break; }
    const cls = text.startsWith('@@') ? 'hunk' : text.startsWith('+') ? 'add' : text.startsWith('-') ? 'rem' : '';
    t.append(h('div', { class: `pl ${cls}` }, h('span', { class: 'ln' }, o ?? ''), h('span', { class: 'ln' }, n ?? ''), h('span', { class: 'code' }, cls === 'hunk' ? text : text.slice(1) || ' ')));
  }
  return t;
}

async function gitAct(m, p, ok) {
  try {
    S.git = { ...(await conn.call(m, { worktree: S.git.worktree, ...p })), worktree: S.git.worktree };
    toast(ok);
    renderDiff();
    renderStatus();
  } catch (e) { toast(e.message, 'error', 6000); }
}

async function createPr() {
  try {
    toast('Pushing and opening a pull request…');
    const r = await conn.call('git.pr', { worktree: S.git.worktree });
    toast('Pull request created');
    if (r.url) window.open(r.url, '_blank', 'noopener');
  } catch (e) { toast(e.message, 'error', 7000); }
}

// ---------- status bar ----------

function renderStatus() {
  const g = S.git;
  const running = allThreads().filter((t) => t.status?.running).length;
  const waiting = allThreads().filter((t) => t.status?.attention).length;
  fill(el.status, 
    h('span', { class: 'st-item' }, icon('branch', 11), g?.branch || S.thread?.branch || '—'),
    g ? h('span', { class: 'st-item' }, `↑${g.ahead} ↓${g.behind}`) : null,
    h('span', { class: 'st-item' }, `${running} running`),
    waiting ? h('span', { class: 'st-item warn' }, `${waiting} need approval`) : null,
    h('div', { class: 'spacer' }),
    h('span', { class: 'st-item' }, S.hello ? `${S.hello.host} · IDE by Insyd ${S.hello.version}` : ''));
}

// ---------- host menu & scripts ----------

async function hostMenu(anchor) {
  const wt = currentWorktree();
  let script = null;
  if (wt) script = await conn.call('script', { worktree: wt }).catch(() => null);
  menu(anchor, [
    { header: `${S.hello?.host || ''} · ${conn.state === 'open' ? 'connected' : conn.state}` },
    script ? { label: script.label, hint: script.command, icon: icon('play', 13), run: () => { toggleTerm(true); term.open(script.command); } } : null,
    { label: 'Open a terminal', icon: glyph('terminal', 14), run: () => { toggleTerm(true); term.open(); } },
    { label: 'Refresh projects', icon: icon('refresh', 13), run: () => loadProjects(true) },
    'Notification' in window && Notification.permission === 'default' ? { label: 'Notify me when agents finish', icon: icon('check', 13), run: () => Notification.requestPermission() } : null,
    { sep: true },
    { label: 'Disconnect this browser', danger: true, run: () => { forgetToken(); location.reload(); } },
  ].filter(Boolean), { width: 300 });
}

// ---------- command palette ----------

function palette() {
  dialog((box, close) => {
    const input = h('input', { class: 'palette-input', placeholder: 'Search threads and actions…', autofocus: true });
    const list = h('div', { class: 'palette-list' });
    let sel = 0, rows = [];
    const actions = [
      { label: 'New thread', hint: '⌘⇧O', run: () => newThread(S.draft.project) },
      { label: 'Toggle terminal', hint: '⌘J', run: () => toggleTerm() },
      { label: 'Toggle changes', hint: '⌘⇧G', run: () => toggleDiff() },
      { label: 'Add project', run: addProject },
    ];
    const draw = () => {
      const q = input.value.trim().toLowerCase();
      const threads = allThreads().filter((t) => !q || t.title.toLowerCase().includes(q) || t.project.name.toLowerCase().includes(q)).slice(0, 40)
        .map((t) => ({ label: t.title, hint: `${t.project.name} · ${ago(t.updated)}`, badge: t.agent, run: () => { location.hash = `#/t/${t.id}`; } }));
      rows = [...actions.filter((a) => !q || a.label.toLowerCase().includes(q)), ...threads];
      sel = Math.min(sel, Math.max(0, rows.length - 1));
      fill(list, ...rows.map((r, i) => h('button', { class: `palette-row${i === sel ? ' on' : ''}`, onclick: () => { close(); r.run(); } },
        r.badge ? agentBadge(r.badge, 16) : icon('chevron-right', 12), h('span', { class: 'palette-label' }, r.label), h('span', { class: 'palette-hint' }, r.hint || ''))));
      list.children[sel]?.scrollIntoView({ block: 'nearest' });
    };
    input.addEventListener('input', () => { sel = 0; draw(); });
    input.addEventListener('keydown', (e) => {
      if (e.key === 'ArrowDown') { sel = Math.min(rows.length - 1, sel + 1); draw(); e.preventDefault(); }
      if (e.key === 'ArrowUp') { sel = Math.max(0, sel - 1); draw(); e.preventDefault(); }
      if (e.key === 'Enter' && rows[sel]) { close(); rows[sel].run(); }
    });
    box.classList.add('palette');
    box.append(input, list);
    draw();
  });
}

// ---------- keys ----------

addEventListener('keydown', (e) => {
  const mod = e.metaKey || e.ctrlKey;
  if (!mod || conn.state === 'unpaired') return;
  const k = e.key.toLowerCase();
  if (k === 'k') { e.preventDefault(); closeMenus(); palette(); }
  else if (k === 'j') { e.preventDefault(); toggleTerm(); }
  else if (e.shiftKey && k === 'g') { e.preventDefault(); toggleDiff(); }
  else if (e.shiftKey && k === 'o') { e.preventDefault(); newThread(S.draft.project); }
});

addEventListener('resize', () => applyPanels());
document.addEventListener('visibilitychange', () => {
  if (!document.hidden && conn.state === 'offline') conn.connect();
});

conn.connect();
