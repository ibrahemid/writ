export function escapeHtml(s: string): string {
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

function escapeAttr(s: string): string {
  return escapeHtml(s).replace(/"/g, '&quot;');
}

// Drop hrefs that carry an executable scheme. The editable surface is Markdown,
// so a [label](javascript:…) link would otherwise stay clickable in the preview.
function isSafeUrl(url: string): boolean {
  return !/^\s*(javascript|data|vbscript|file):/i.test(url);
}

const PH_OPEN = String.fromCharCode(0);
const PH_CLOSE = String.fromCharCode(1);

export function renderInline(text: string): string {
  const store: string[] = [];
  const put = (html: string): string => {
    store.push(html);
    return PH_OPEN + (store.length - 1) + PH_CLOSE;
  };
  let t = text;
  t = t.replace(/`([^`]+)`/g, (_m, c: string) => put('<code>' + escapeHtml(c) + '</code>'));
  t = t.replace(/\$\$([^$]+)\$\$/g, (m: string) => put(escapeHtml(m)));
  t = t.replace(/\$([^$\n]+)\$/g, (m: string) => put(escapeHtml(m)));
  t = escapeHtml(t);
  t = t.replace(
    /\[([^\]]+)\]\(([^)]+)\)/g,
    (_m, label: string, url: string) =>
      isSafeUrl(url)
        ? '<a href="' + escapeAttr(url) + '" target="_blank" rel="noopener noreferrer">' + label + '</a>'
        : label,
  );
  t = t.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  t = t.replace(/(^|[^*])\*([^*\n]+)\*/g, '$1<em>$2</em>');
  t = t.replace(/~~([^~]+)~~/g, '<del>$1</del>');
  t = t.replace(
    new RegExp(PH_OPEN + '(\\d+)' + PH_CLOSE, 'g'),
    (_m, i: string) => store[+i] ?? '',
  );
  return t;
}

export function mdToHtml(src: string): string {
  const lines = String(src).replace(/\r\n/g, '\n').split('\n');
  const at = (k: number): string => lines[k] ?? '';
  const out: string[] = [];
  let i = 0;
  let taskIndex = 0;
  while (i < lines.length) {
    const line = at(i);

    const fence = line.match(/^```\s*([\w-]+)?\s*$/);
    if (fence) {
      const lang = fence[1] || '';
      i++;
      const code: string[] = [];
      while (i < lines.length && !/^```\s*$/.test(at(i))) {
        code.push(at(i));
        i++;
      }
      i++;
      const body = code.join('\n');
      if (lang === 'mermaid') {
        out.push(
          '<pre class="mermaid" data-src="' + escapeAttr(body) + '">' + escapeHtml(body) + '</pre>',
        );
      } else {
        out.push('<pre><code>' + escapeHtml(body) + '</code></pre>');
      }
      continue;
    }

    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) {
      const level = (heading[1] ?? '').length;
      out.push('<h' + level + '>' + renderInline(heading[2] ?? '') + '</h' + level + '>');
      i++;
      continue;
    }

    if (/^(-{3,}|\*{3,})\s*$/.test(line)) {
      out.push('<hr>');
      i++;
      continue;
    }

    if (/^>\s?/.test(line)) {
      const quote: string[] = [];
      while (i < lines.length && /^>\s?/.test(at(i))) {
        quote.push(at(i).replace(/^>\s?/, ''));
        i++;
      }
      out.push('<blockquote>' + renderInline(quote.join(' ')) + '</blockquote>');
      continue;
    }

    if (
      line.includes('|') &&
      i + 1 < lines.length &&
      /^\s*\|?[\s:\-|]+\|?\s*$/.test(at(i + 1)) &&
      at(i + 1).includes('-')
    ) {
      const parseRow = (r: string): string[] =>
        r
          .replace(/^\s*\|/, '')
          .replace(/\|\s*$/, '')
          .split('|')
          .map((c) => c.trim());
      const heads = parseRow(line);
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && at(i).includes('|') && at(i).trim() !== '') {
        rows.push(parseRow(at(i)));
        i++;
      }
      out.push(
        '<table><thead><tr>' +
          heads.map((h) => '<th>' + renderInline(h) + '</th>').join('') +
          '</tr></thead><tbody>' +
          rows
            .map((r) => '<tr>' + r.map((c) => '<td>' + renderInline(c) + '</td>').join('') + '</tr>')
            .join('') +
          '</tbody></table>',
      );
      continue;
    }

    if (/^\s*[-*+]\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*[-*+]\s+/.test(at(i))) {
        const item = at(i).replace(/^\s*[-*+]\s+/, '');
        const task = item.match(/^\[([ xX])\]\s+(.*)$/);
        if (task) {
          const checked = (task[1] ?? '').toLowerCase() === 'x';
          // data-task carries the source-order index (a number we control, never
          // user text); the click delegate flips the matching line in the buffer.
          items.push(
            '<li class="task"><button type="button" class="ck' +
              (checked ? ' on' : '') +
              '" data-task="' +
              taskIndex +
              '" role="checkbox" aria-checked="' +
              (checked ? 'true' : 'false') +
              '">' +
              (checked ? '✓' : '') +
              '</button><span>' +
              renderInline(task[2] ?? '') +
              '</span></li>',
          );
          taskIndex += 1;
        } else {
          items.push('<li>' + renderInline(item) + '</li>');
        }
        i++;
      }
      out.push('<ul>' + items.join('') + '</ul>');
      continue;
    }

    if (/^\s*\d+\.\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*\d+\.\s+/.test(at(i))) {
        items.push('<li>' + renderInline(at(i).replace(/^\s*\d+\.\s+/, '')) + '</li>');
        i++;
      }
      out.push('<ol>' + items.join('') + '</ol>');
      continue;
    }

    if (line.trim() === '') {
      i++;
      continue;
    }

    const buf: string[] = [];
    while (
      i < lines.length &&
      at(i).trim() !== '' &&
      !/^(#{1,6})\s|^```|^>\s?|^\s*[-*+]\s+|^\s*\d+\.\s+/.test(at(i)) &&
      !/^(-{3,}|\*{3,})\s*$/.test(at(i))
    ) {
      buf.push(at(i));
      i++;
    }
    if (buf.length) out.push('<p>' + renderInline(buf.join(' ')) + '</p>');
  }
  return out.join('\n');
}

export function estimateTokens(text: string): number {
  return Math.max(1, Math.round(text.length / 4));
}

export function formatTokens(n: number): string {
  return n >= 1000 ? Math.round(n / 1000) + 'k' : '' + n;
}
