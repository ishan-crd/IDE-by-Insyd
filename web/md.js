// Markdown for agent replies. Everything is HTML-escaped first; only the
// constructs below produce markup, and links are limited to http(s)/mailto.
// Small on purpose: chat replies use a narrow slice of Markdown.

const esc = (s) => s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

function safeUrl(u) {
  const t = u.trim();
  return /^(https?:\/\/|mailto:)/i.test(t) ? t : null;
}

function inline(src) {
  // Code spans first, parked so nothing inside them is formatted.
  const codes = [];
  let s = src.replace(/(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)/g, (_, _t, c) => {
    codes.push(`<code>${esc(c.trim() ? c.replace(/^ (.*) $/, '$1') : c)}</code>`);
    return `\u0000${codes.length - 1}\u0000`;
  });
  const links = [];
  s = s.replace(/\[([^\]\n]+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g, (m, text, url) => {
    const u = safeUrl(url);
    if (!u) return m;
    links.push(`<a href="${esc(u)}" target="_blank" rel="noopener noreferrer">${esc(text)}</a>`);
    return `\u0001${links.length - 1}\u0001`;
  });
  s = esc(s);
  s = s.replace(/(^|[\s(])(https?:\/\/[^\s<)]+[^\s<).,;:!?'"])/g, (_, pre, url) =>
    `${pre}<a href="${url}" target="_blank" rel="noopener noreferrer">${url}</a>`);
  s = s.replace(/\*\*([^*\n]+?)\*\*/g, '<strong>$1</strong>')
    .replace(/__([^_\n]+?)__/g, '<strong>$1</strong>')
    .replace(/(^|[^*\w])\*([^*\n]+?)\*(?!\w)/g, '$1<em>$2</em>')
    .replace(/(^|[^_\w])_([^_\n]+?)_(?!\w)/g, '$1<em>$2</em>')
    .replace(/~~([^~\n]+?)~~/g, '<del>$1</del>');
  s = s.replace(/\u0001(\d+)\u0001/g, (_, i) => links[+i]);
  return s.replace(/\u0000(\d+)\u0000/g, (_, i) => codes[+i]);
}

function table(lines) {
  const cells = (l) => l.trim().replace(/^\||\|$/g, '').split('|').map((c) => c.trim());
  const head = cells(lines[0]);
  const aligns = cells(lines[1]).map((c) => (c.startsWith(':') && c.endsWith(':') ? 'center' : c.endsWith(':') ? 'right' : ''));
  const th = head.map((c, i) => `<th${aligns[i] ? ` style="text-align:${aligns[i]}"` : ''}>${inline(c)}</th>`).join('');
  const rows = lines.slice(2).map((l) => `<tr>${cells(l).map((c, i) => `<td${aligns[i] ? ` style="text-align:${aligns[i]}"` : ''}>${inline(c)}</td>`).join('')}</tr>`).join('');
  return `<div class="md-table"><table><thead><tr>${th}</tr></thead><tbody>${rows}</tbody></table></div>`;
}

function list(lines, start) {
  // Items at the first line's indent; deeper lines belong to the item above.
  const indent = (l) => l.match(/^\s*/)[0].replace(/\t/g, '    ').length;
  const base = indent(lines[start]);
  const ordered = /^\s*\d+[.)]\s/.test(lines[start]);
  const items = [];
  let i = start;
  while (i < lines.length) {
    const l = lines[i];
    if (!l.trim()) {
      // A blank line ends the list unless the next line continues it.
      const next = lines[i + 1];
      if (next && indent(next) >= base && /^\s*([-*+]|\d+[.)])\s/.test(next)) { i++; continue; }
      break;
    }
    const m = l.match(/^(\s*)([-*+]|\d+[.)])\s+(.*)$/);
    if (m && indent(l) === base) {
      items.push({ text: m[3], sub: [] });
    } else if (items.length && indent(l) > base) {
      items[items.length - 1].sub.push(l);
    } else break;
    i++;
  }
  const num = ordered ? parseInt(lines[start].trim(), 10) : 1;
  const tag = ordered ? 'ol' : 'ul';
  const lis = items.map((it) => {
    let text = it.text;
    let check = '';
    const task = text.match(/^\[([ xX])\]\s+(.*)$/);
    if (task) { check = `<span class="md-check${task[1] !== ' ' ? ' done' : ''}"></span>`; text = task[2]; }
    const sub = it.sub.length ? render(it.sub.join('\n')) : '';
    return `<li${task ? ' class="task"' : ''}>${check}${inline(text)}${sub}</li>`;
  }).join('');
  return { html: `<${tag}${ordered && num !== 1 ? ` start="${num}"` : ''}>${lis}</${tag}>`, end: i };
}

export function render(src) {
  const lines = String(src || '').replace(/\r\n?/g, '\n').split('\n');
  let out = '';
  let para = [];
  const flush = () => {
    if (para.length) out += `<p>${para.map(inline).join('<br>')}</p>`;
    para = [];
  };
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    const fence = l.match(/^\s*(```+|~~~+)\s*([\w+#.-]*)/);
    if (fence) {
      flush();
      const close = fence[1];
      const body = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith(close)) body.push(lines[i++]);
      const lang = fence[2] ? `<span class="code-lang">${esc(fence[2])}</span>` : '';
      out += `<div class="code">${lang}<button class="code-copy" data-copy title="Copy">Copy</button><pre><code>${esc(body.join('\n'))}</code></pre></div>`;
      continue;
    }
    if (!l.trim()) { flush(); continue; }
    const hd = l.match(/^(#{1,6})\s+(.*?)\s*#*\s*$/);
    if (hd) { flush(); out += `<h${hd[1].length}>${inline(hd[2])}</h${hd[1].length}>`; continue; }
    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(l)) { flush(); out += '<hr>'; continue; }
    if (/^\s*>/.test(l)) {
      flush();
      const q = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) q.push(lines[i++].replace(/^\s*>\s?/, ''));
      i--;
      out += `<blockquote>${render(q.join('\n'))}</blockquote>`;
      continue;
    }
    if (/^\s*([-*+]|\d+[.)])\s+/.test(l)) {
      flush();
      const r = list(lines, i);
      out += r.html;
      i = r.end - 1;
      continue;
    }
    if (l.includes('|') && i + 1 < lines.length && /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/.test(lines[i + 1])) {
      flush();
      const t = [l, lines[i + 1]];
      i += 2;
      while (i < lines.length && lines[i].includes('|') && lines[i].trim()) t.push(lines[i++]);
      i--;
      out += table(t);
      continue;
    }
    para.push(l);
  }
  flush();
  return out;
}

export { esc };
