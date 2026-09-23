// Small DOM helpers shared by the web client. No framework: views build
// elements with `h()` and update only what changed.

export function h(tag, attrs, ...children) {
  const el = document.createElement(tag);
  if (attrs) {
    for (const [k, v] of Object.entries(attrs)) {
      if (v == null || v === false) continue;
      if (k === 'class') el.className = v;
      else if (k === 'style' && typeof v === 'object') Object.assign(el.style, v);
      else if (k.startsWith('on') && typeof v === 'function') el.addEventListener(k.slice(2), v);
      else if (k === 'html') el.innerHTML = v;
      else if (v === true) el.setAttribute(k, '');
      else el.setAttribute(k, v);
    }
  }
  append(el, children);
  return el;
}

function append(el, children) {
  for (const c of children) {
    if (c == null || c === false) continue;
    if (Array.isArray(c)) append(el, c);
    else el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
}

/** Replace an element's children, skipping empty (null/false) slots. */
export function fill(el, ...kids) {
  el.replaceChildren(...kids.flat().filter((k) => k != null && k !== false));
  return el;
}

/** One-color icon from /icons, tinted with the current text color. */
export function icon(name, size = 16, cls = '') {
  const el = document.createElement('span');
  el.className = `i ${cls}`;
  el.style.width = el.style.height = `${size}px`;
  el.style.setProperty('--src', `url(/icons/${name}.svg)`);
  return el;
}

const LOGOS = { claude: 'claude', codex: 'codex', opencode: 'opencode', pi: 'pi', omp: 'pi', cursor: 'cursor', grok: 'grok', antigravity: 'antigravity' };
const TINTS = { claude: '#E0733A', codex: '#3B7DD8', opencode: '#5E5E58', pi: '#D8A23A', omp: '#7C5CD6', cursor: '#706F6A', grok: '#3D3D3A', antigravity: '#2A9D9F' };

/** Round agent badge with the agent's logo (same as the desktop app). */
export function agentBadge(key, size = 20, mono = '') {
  const el = h('span', { class: 'badge', title: key });
  el.style.width = el.style.height = `${size}px`;
  el.style.background = TINTS[key] || 'var(--hover-2)';
  if (LOGOS[key]) el.append(icon(`agents/${LOGOS[key]}`, Math.round(size * 0.58)));
  else el.append(h('b', { style: { fontSize: `${Math.max(9, size * 0.42)}px` } }, mono || key[0]?.toUpperCase() || '?'));
  return el;
}

/** Inline SVGs for glyphs the desktop icon set doesn't have. */
const EXTRA = {
  menu: '<path d="M2.5 4.5h11M2.5 8h11M2.5 11.5h11" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>',
  terminal: '<rect x="1.5" y="2.5" width="13" height="11" rx="2" stroke="currentColor" stroke-width="1.3" fill="none"/><path d="M4.5 6l2 2-2 2M8 10.5h3.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" fill="none"/>',
  diff: '<path d="M4 2.5v7M1.5 6h5M9.5 12.5h5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/><path d="M11 3l2.5 2.5M13.5 3L11 5.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>',
  commit: '<circle cx="8" cy="8" r="2.6" stroke="currentColor" stroke-width="1.3" fill="none"/><path d="M1.5 8h3.9M10.6 8h3.9" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>',
  upload: '<path d="M8 11V3M4.8 6.2L8 3l3.2 3.2M3 13h10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" fill="none"/>',
  expand: '<path d="M9.5 2.5h4v4M6.5 13.5h-4v-4M13.5 2.5L9 7M2.5 13.5L7 9" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" fill="none"/>',
  dots: '<circle cx="3.5" cy="8" r="1.2" fill="currentColor"/><circle cx="8" cy="8" r="1.2" fill="currentColor"/><circle cx="12.5" cy="8" r="1.2" fill="currentColor"/>',
  keyboard: '<rect x="1.5" y="4" width="13" height="8.5" rx="1.6" stroke="currentColor" stroke-width="1.2" fill="none"/><path d="M4 7h.01M6.5 7h.01M9 7h.01M11.5 7h.01M5 10h6" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>',
  link: '<path d="M6.5 9.5l3-3M7 4.5l1-1a2.5 2.5 0 013.5 3.5l-1 1M9 11.5l-1 1a2.5 2.5 0 01-3.5-3.5l1-1" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" fill="none"/>',
};

export function glyph(name, size = 16) {
  const el = document.createElement('span');
  el.className = 'g';
  el.innerHTML = `<svg width="${size}" height="${size}" viewBox="0 0 16 16" fill="none">${EXTRA[name] || ''}</svg>`;
  return el;
}

export function ago(ts) {
  if (!ts) return '';
  const s = Math.max(0, Date.now() / 1000 - ts);
  if (s < 60) return 'now';
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  if (s < 86400 * 30) return `${Math.floor(s / 86400)}d`;
  return new Date(ts * 1000).toLocaleDateString();
}

export function fmtTokens(n) {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${Math.round(n / 1e3)}k`;
  return String(n);
}

export function secs(n) {
  n = Math.max(0, Math.round(n));
  if (n < 60) return `${n}s`;
  const m = Math.floor(n / 60);
  if (m < 60) return `${m}m ${n % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

export function basename(p) {
  return String(p || '').replace(/\/+$/, '').split('/').pop() || p;
}

let toastBox;
export function toast(text, kind = 'info', ms = 3200) {
  if (!toastBox) { toastBox = h('div', { class: 'toasts' }); document.body.append(toastBox); }
  const t = h('div', { class: `toast ${kind}` }, text);
  toastBox.append(t);
  setTimeout(() => { t.classList.add('out'); setTimeout(() => t.remove(), 250); }, ms);
}

/** A popover menu anchored to `anchor`. Items: {label, hint, icon, danger, on, sep}. */
export function menu(anchor, items, { align = 'left', width } = {}) {
  closeMenus();
  const box = h('div', { class: 'menu', role: 'menu' });
  if (width) box.style.width = `${width}px`;
  for (const it of items) {
    if (it.sep) { box.append(h('div', { class: 'menu-sep' })); continue; }
    if (it.header) { box.append(h('div', { class: 'menu-head' }, it.header)); continue; }
    const row = h('button', { class: `menu-item${it.danger ? ' danger' : ''}${it.on ? '' : ''}${it.checked ? ' checked' : ''}`, role: 'menuitem', disabled: it.disabled },
      it.icon || null, h('span', { class: 'menu-label' }, it.label), it.hint ? h('span', { class: 'menu-hint' }, it.hint) : null,
      it.checked ? icon('check', 13, 'menu-check') : null);
    row.addEventListener('click', (e) => { e.stopPropagation(); closeMenus(); it.run?.(); });
    box.append(row);
  }
  document.body.append(box);
  const r = anchor.getBoundingClientRect();
  const bw = box.offsetWidth, bh = box.offsetHeight;
  let x = align === 'right' ? r.right - bw : r.left;
  let y = r.bottom + 6;
  if (y + bh > innerHeight - 8) y = Math.max(8, r.top - bh - 6);
  x = Math.max(8, Math.min(x, innerWidth - bw - 8));
  box.style.left = `${x}px`;
  box.style.top = `${y}px`;
  setTimeout(() => document.addEventListener('pointerdown', outside, true), 0);
  function outside(e) { if (!box.contains(e.target)) closeMenus(); }
  box._off = () => document.removeEventListener('pointerdown', outside, true);
  return box;
}

export function closeMenus() {
  for (const m of document.querySelectorAll('.menu')) { m._off?.(); m.remove(); }
}

/** Modal dialog; resolves with the value passed to `close`. */
export function dialog(build, { wide = false } = {}) {
  return new Promise((resolve) => {
    const shade = h('div', { class: 'shade' });
    const box = h('div', { class: `dialog${wide ? ' wide' : ''}`, role: 'dialog' });
    const close = (v) => { shade.remove(); document.removeEventListener('keydown', esc, true); resolve(v); };
    function esc(e) { if (e.key === 'Escape') { e.stopPropagation(); close(null); } }
    document.addEventListener('keydown', esc, true);
    shade.addEventListener('pointerdown', (e) => { if (e.target === shade) close(null); });
    build(box, close);
    shade.append(box);
    document.body.append(shade);
    box.querySelector('[autofocus]')?.focus();
  });
}

export async function prompt(title, { value = '', placeholder = '', ok = 'Save' } = {}) {
  return dialog((box, close) => {
    const input = h('input', { class: 'field', value, placeholder, autofocus: true });
    input.addEventListener('keydown', (e) => { if (e.key === 'Enter') close(input.value); });
    box.append(h('div', { class: 'dialog-title' }, title), input,
      h('div', { class: 'dialog-actions' },
        h('button', { class: 'btn', onclick: () => close(null) }, 'Cancel'),
        h('button', { class: 'btn primary', onclick: () => close(input.value) }, ok)));
    setTimeout(() => input.select(), 0);
  });
}

export async function confirmBox(title, body, ok = 'Delete') {
  return dialog((box, close) => {
    box.append(h('div', { class: 'dialog-title' }, title), h('p', { class: 'dialog-body' }, body),
      h('div', { class: 'dialog-actions' },
        h('button', { class: 'btn', onclick: () => close(false), autofocus: true }, 'Cancel'),
        h('button', { class: 'btn danger', onclick: () => close(true) }, ok)));
  });
}
