// Terminal drawer: live shells on the InsyDE machine, rendered by xterm.js.
// Terminals belong to a worktree and keep running on the server when the
// drawer closes or the tab disconnects; reattaching replays recent output.

import { Terminal } from '/vendor/xterm.mjs';
import { FitAddon } from '/vendor/addon-fit.mjs';
import { WebLinksAddon } from '/vendor/addon-web-links.mjs';
import { h, icon, glyph, toast } from '/ui.js';

const PALETTE = {
  dark: ['#3A3A38', '#F0A0A4', '#3ECF8E', '#F2C14E', '#3B7DD8', '#7C5CD6', '#2A9D9F', '#C9C9C5',
    '#9C9C97', '#FF8A8F', '#6BE3AE', '#F7D06E', '#7FA8F0', '#A88CF0', '#5CC6C8', '#F2F2F0'],
  light: ['#1A1A19', '#B3261E', '#2E9E6B', '#D8A23A', '#3B7DD8', '#7C5CD6', '#2A9D9F', '#3D3D3A',
    '#706F6A', '#D6453C', '#2E9E6B', '#B7861F', '#2F6BEB', '#7C5CD6', '#2A9D9F', '#1A1A19'],
};
const NAMES = ['black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white'];

function theme() {
  const css = getComputedStyle(document.documentElement);
  const v = (n) => css.getPropertyValue(n).trim();
  const dark = document.documentElement.dataset.theme !== 'light';
  const p = PALETTE[dark ? 'dark' : 'light'];
  const t = { background: v('--panel-2'), foreground: v('--ink-2'), cursor: v('--ink'), cursorAccent: v('--panel-2'), selectionBackground: v('--sel-chip') };
  NAMES.forEach((n, i) => { t[n] = p[i]; t[`bright${n[0].toUpperCase()}${n.slice(1)}`] = p[i + 8]; });
  return t;
}

const SEQ = { Esc: '\x1b', Tab: '\t', '↑': '\x1b[A', '↓': '\x1b[B', '→': '\x1b[C', '←': '\x1b[D', '^C': '\x03', '^D': '\x04', '^L': '\x0c', '^R': '\x12' };

export class TermDrawer {
  constructor(conn, { onChange }) {
    this.conn = conn;
    this.onChange = onChange;
    this.terms = new Map(); // id -> {id, title, alive, worktree, xterm, fit, host}
    this.worktree = null;
    this.active = null;
    this.ctrl = false;
    this.el = h('div', { class: 'term-drawer' });
    this.tabs = h('div', { class: 'term-tabs' });
    this.body = h('div', { class: 'term-body' });
    this.keys = this.keyBar();
    this.el.append(this.tabs, this.body, this.keys);
    conn.binary = (u8) => {
      if (u8[0] !== 1) return;
      const id = (u8[1] << 24) | (u8[2] << 16) | (u8[3] << 8) | u8[4];
      this.terms.get(id)?.xterm.write(u8.subarray(5));
    };
    conn.on('term.exit', (e) => {
      const t = this.terms.get(e.id);
      if (t) { t.alive = false; t.xterm.write('\r\n\x1b[2m[process exited]\x1b[0m\r\n'); this.renderTabs(); }
    });
    conn.on('terms', () => this.worktree && this.load(this.worktree, false));
    this.ro = new ResizeObserver(() => this.fitActive());
    this.ro.observe(this.body);
  }

  /** Show the terminals of `worktree`, opening one if there are none. */
  async load(worktree, autostart = true) {
    this.worktree = worktree;
    let list = [];
    try { list = await this.conn.call('term.list', { worktree }); } catch { return; }
    const known = new Set(list.map((t) => t.id));
    for (const t of list) {
      if (!this.terms.has(t.id)) await this.attach({ ...t, worktree });
    }
    // Forget terminals that were closed elsewhere.
    for (const [id, t] of this.terms) {
      if (t.worktree === worktree && !known.has(id)) this.drop(id);
    }
    const mine = [...this.terms.values()].filter((t) => t.worktree === worktree);
    if (!mine.length && autostart) { await this.open(); return; }
    if (!mine.some((t) => t.id === this.active)) this.active = mine[mine.length - 1]?.id ?? null;
    this.renderTabs();
    this.show();
  }

  make(meta) {
    const xterm = new Terminal({
      fontFamily: 'Menlo, "SF Mono", ui-monospace, monospace',
      fontSize: innerWidth < 700 ? 11 : 12,
      lineHeight: 1.25,
      cursorBlink: true,
      scrollback: 5000,
      allowProposedApi: false,
      theme: theme(),
      macOptionIsMeta: true,
    });
    const fit = new FitAddon();
    xterm.loadAddon(fit);
    xterm.loadAddon(new WebLinksAddon((_, uri) => window.open(uri, '_blank', 'noopener')));
    const host = h('div', { class: 'term-host' });
    const t = { ...meta, xterm, fit, host };
    xterm.onData((data) => {
      if (this.ctrl && data.length === 1) {
        const c = data.toUpperCase().charCodeAt(0);
        if (c >= 64 && c <= 95) data = String.fromCharCode(c - 64);
        this.setCtrl(false);
      }
      this.conn.send('term.input', { id: t.id, data });
    });
    xterm.onResize(({ cols, rows }) => this.conn.send('term.resize', { id: t.id, cols, rows }));
    this.terms.set(t.id, t);
    return t;
  }

  async attach(meta) {
    const t = this.terms.get(meta.id) || this.make(meta);
    t.xterm.reset();
    try {
      const r = await this.conn.call('term.attach', { id: meta.id });
      t.alive = r.alive;
    } catch { this.drop(meta.id); }
  }

  /** After a reconnect: re-subscribe every terminal we still show. */
  async reattach() {
    for (const t of [...this.terms.values()]) await this.attach(t);
    if (this.worktree) await this.load(this.worktree, false);
  }

  async open(command) {
    if (!this.worktree) return;
    const { cols, rows } = this.estimate();
    try {
      const r = await this.conn.call('term.open', { worktree: this.worktree, cols, rows, command });
      await this.attach({ id: r.id, title: r.title, alive: true, worktree: this.worktree });
      this.active = r.id;
      this.renderTabs();
      this.show();
      this.onChange?.();
    } catch (e) { toast(e.message, 'error'); }
  }

  async close(id) {
    try { await this.conn.call('term.close', { id }); } catch {}
    this.drop(id);
    const mine = [...this.terms.values()].filter((t) => t.worktree === this.worktree);
    this.active = mine[mine.length - 1]?.id ?? null;
    this.renderTabs();
    this.show();
  }

  drop(id) {
    const t = this.terms.get(id);
    if (!t) return;
    t.xterm.dispose();
    t.host.remove();
    this.terms.delete(id);
  }

  estimate() {
    const r = this.body.getBoundingClientRect();
    return { cols: Math.max(20, Math.floor((r.width || 800) / 7.3)), rows: Math.max(5, Math.floor((r.height || 240) / 15)) };
  }

  renderTabs() {
    this.tabs.replaceChildren();
    const mine = [...this.terms.values()].filter((t) => t.worktree === this.worktree);
    for (const t of mine) {
      const tab = h('div', { class: `term-tab${t.id === this.active ? ' on' : ''}${t.alive === false ? ' dead' : ''}`, onclick: () => { this.active = t.id; this.renderTabs(); this.show(); } },
        h('span', { class: 'term-dot' }), h('span', { class: 'term-title' }, t.title || 'shell'),
        h('button', { class: 'term-x', title: 'Close terminal', onclick: (e) => { e.stopPropagation(); this.close(t.id); } }, icon('close', 11)));
      this.tabs.append(tab);
    }
    this.tabs.append(h('button', { class: 'term-add', title: 'New terminal', onclick: () => this.open() }, icon('plus', 13), h('span', null, 'Terminal')));
  }

  show() {
    const t = this.terms.get(this.active);
    for (const x of this.terms.values()) x.host.classList.toggle('on', x === t);
    if (!t) return;
    if (!t.host.isConnected) {
      this.body.append(t.host);
      t.xterm.open(t.host);
    }
    requestAnimationFrame(() => { this.fitActive(); t.xterm.focus(); });
  }

  fitActive() {
    const t = this.terms.get(this.active);
    if (!t || !t.host.isConnected || !this.body.offsetHeight) return;
    try { t.fit.fit(); } catch {}
  }

  refreshTheme() {
    const th = theme();
    for (const t of this.terms.values()) t.xterm.options.theme = th;
  }

  setCtrl(on) {
    this.ctrl = on;
    this.keys.querySelector('[data-k="Ctrl"]')?.classList.toggle('on', on);
  }

  keyBar() {
    const bar = h('div', { class: 'term-keys' });
    for (const k of ['Esc', 'Tab', 'Ctrl', '^C', '↑', '↓', '←', '→', '|', '~', '/', '-']) {
      const b = h('button', { 'data-k': k }, k);
      b.addEventListener('pointerdown', (e) => e.preventDefault()); // keep the keyboard up
      b.addEventListener('click', () => {
        const t = this.terms.get(this.active);
        if (!t) return;
        if (k === 'Ctrl') { this.setCtrl(!this.ctrl); return; }
        this.conn.send('term.input', { id: t.id, data: SEQ[k] ?? k });
        t.xterm.focus();
      });
      bar.append(b);
    }
    bar.append(h('span', { class: 'term-keys-hint' }, glyph('keyboard', 14)));
    return bar;
  }
}
