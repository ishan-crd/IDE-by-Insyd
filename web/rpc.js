// Connection to the InsyDE server: one WebSocket, JSON requests with ids,
// pushed events, and binary terminal frames. Reconnects on its own.

const TOKEN_KEY = 'insyde.token';

export function takeTokenFromUrl() {
  const m = location.hash.match(/(?:^#|&)t=([0-9a-f]{32,128})/i);
  if (m) {
    try { localStorage.setItem(TOKEN_KEY, m[1]); } catch {}
    history.replaceState(null, '', location.pathname + location.search);
  }
}

export function getToken() {
  try { return localStorage.getItem(TOKEN_KEY) || ''; } catch { return ''; }
}

export function setToken(t) {
  try { localStorage.setItem(TOKEN_KEY, t); } catch {}
}

export function forgetToken() {
  try { localStorage.removeItem(TOKEN_KEY); } catch {}
}

export class Connection {
  constructor() {
    this.ws = null;
    this.seq = 1;
    this.pending = new Map();
    this.handlers = new Map();
    this.binary = null;
    this.state = 'connecting'; // connecting | open | offline | unpaired
    this.retry = 0;
    this.everOpened = false;
  }

  on(ev, fn) {
    if (!this.handlers.has(ev)) this.handlers.set(ev, new Set());
    this.handlers.get(ev).add(fn);
    return () => this.handlers.get(ev).delete(fn);
  }

  emit(ev, data) {
    for (const fn of this.handlers.get(ev) || []) fn(data);
  }

  setState(s) {
    if (this.state === s) return;
    this.state = s;
    this.emit('state', s);
  }

  async connect() {
    const token = getToken();
    if (!token) { this.setState('unpaired'); return; }
    this.setState(this.everOpened ? 'offline' : 'connecting');
    // Check the pairing over plain HTTP first: a refused WebSocket can't say why.
    try {
      const r = await fetch(`/auth?t=${encodeURIComponent(token)}`, { cache: 'no-store' });
      if (r.status === 401) { this.setState('unpaired'); return; }
    } catch {
      const wait = Math.min(8000, 400 * 2 ** this.retry++);
      setTimeout(() => this.connect(), wait);
      return;
    }
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const ws = new WebSocket(`${proto}//${location.host}/ws?t=${encodeURIComponent(token)}`);
    ws.binaryType = 'arraybuffer';
    this.ws = ws;
    let opened = false;
    ws.onopen = () => {
      opened = true;
      this.everOpened = true;
      this.retry = 0;
      this.setState('open');
      this.emit('open');
    };
    ws.onmessage = (e) => {
      if (typeof e.data !== 'string') {
        if (this.binary) this.binary(new Uint8Array(e.data));
        return;
      }
      let msg;
      try { msg = JSON.parse(e.data); } catch { return; }
      if (msg.ev) { this.emit(msg.ev, msg); return; }
      const p = this.pending.get(msg.id);
      if (!p) return;
      this.pending.delete(msg.id);
      if ('err' in msg) p.reject(new Error(msg.err)); else p.resolve(msg.ok);
    };
    ws.onclose = () => {
      for (const p of this.pending.values()) p.reject(new Error('Disconnected'));
      this.pending.clear();
      if (this.ws !== ws) return;
      this.ws = null;
      this.setState(opened || this.everOpened ? 'offline' : 'connecting');
      const wait = Math.min(8000, 400 * 2 ** this.retry++);
      setTimeout(() => this.connect(), wait);
    };
  }

  /** Call a server method; resolves with its result. */
  call(m, p = {}) {
    return new Promise((resolve, reject) => {
      if (!this.ws || this.ws.readyState !== 1) { reject(new Error('Not connected')); return; }
      const id = this.seq++;
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, m, p }));
    });
  }

  /** Fire-and-forget (keystrokes, resizes). */
  send(m, p = {}) {
    if (this.ws && this.ws.readyState === 1) this.ws.send(JSON.stringify({ m, p }));
  }
}
