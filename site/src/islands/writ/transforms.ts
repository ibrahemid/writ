export type TransformId =
  | 'trim'
  | 'dedent'
  | 'finalnl'
  | 'prompt'
  | 'tidy'
  | 'normalize'
  | 'punct'
  | 'quotes';

function dedent(s: string): string {
  const lines = s.split('\n');
  let min = Infinity;
  for (const line of lines) {
    if (line.trim() === '') continue;
    const indent = line.match(/^[ \t]*/)![0].length;
    if (indent < min) min = indent;
  }
  if (!isFinite(min) || min === 0) return s;
  return lines.map((line) => line.slice(min)).join('\n');
}

export function applyTransform(id: TransformId, s: string): string {
  const lines = (): string[] => s.split('\n');
  if (id === 'trim') return lines().map((l) => l.replace(/^[ \t]+/, '')).join('\n');
  if (id === 'finalnl') return s.replace(/\s*$/, '') + '\n';
  if (id === 'normalize') {
    return lines()
      .map((l) => {
        const m = l.match(/^([ \t]*)(.*)$/);
        const indent = m?.[1] ?? '';
        const rest = m?.[2] ?? l;
        return indent + rest.replace(/[ \t]{2,}/g, ' ');
      })
      .join('\n');
  }
  if (id === 'punct') return s.replace(/[ \t]+([,.;:!?])/g, '$1');
  if (id === 'quotes') {
    return s
      .replace(/[“”„″]/g, '"')
      .replace(/[‘’‚′]/g, "'")
      .replace(/[–—]/g, '-');
  }
  if (id === 'dedent') return dedent(s);
  if (id === 'tidy') {
    let r = dedent(s);
    r = r
      .split('\n')
      .map((l) => l.replace(/[ \t]+$/, ''))
      .join('\n');
    r = r.replace(/\n{3,}/g, '\n\n');
    return r.replace(/\s*$/, '') + '\n';
  }
  if (id === 'prompt') {
    let r = s.replace(/^---\n[\s\S]*?\n---\n?/, '');
    const parts = r.split(/(```[\s\S]*?```)/);
    r = parts.map((p) => (p.indexOf('```') === 0 ? p : p.replace(/<!--[\s\S]*?-->/g, ''))).join('');
    r = r
      .split('\n')
      .map((l) => l.replace(/[ \t]+$/, ''))
      .join('\n');
    return r.replace(/\n{3,}/g, '\n\n').replace(/\s*$/, '') + '\n';
  }
  return s;
}
