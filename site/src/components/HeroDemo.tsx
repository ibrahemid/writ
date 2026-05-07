import { useEffect, useRef, useState } from 'react';
import { motion, useReducedMotion } from 'framer-motion';

type Tok = { text: string; kind?: 'comment' | 'keyword' | 'string' | 'accent' | 'fn' | 'punct' | 'plain' };
type Line = { indent?: number; tokens: Tok[] };

const SCRIPT: Line[] = [
  { tokens: [{ text: '// scratchpad — always a keypress away', kind: 'comment' }] },
  { tokens: [
    { text: 'use', kind: 'keyword' }, { text: ' writ::', kind: 'plain' },
    { text: 'Scratchpad', kind: 'fn' }, { text: ';', kind: 'punct' },
  ]},
  { tokens: [{ text: '', kind: 'plain' }] },
  { tokens: [
    { text: 'fn', kind: 'keyword' }, { text: ' ' },
    { text: 'main', kind: 'fn' }, { text: '() {', kind: 'punct' },
  ]},
  { indent: 1, tokens: [
    { text: 'let', kind: 'keyword' }, { text: ' ' },
    { text: 'pad', kind: 'accent' }, { text: ' = ', kind: 'plain' },
    { text: 'Scratchpad::', kind: 'fn' }, { text: 'new', kind: 'fn' }, { text: '()', kind: 'punct' },
  ]},
  { indent: 2, tokens: [
    { text: '.hotkey(', kind: 'plain' }, { text: '"Cmd+Shift+Space"', kind: 'string' }, { text: ')', kind: 'plain' },
  ]},
  { indent: 2, tokens: [
    { text: '.autosave(', kind: 'plain' }, { text: 'true', kind: 'keyword' }, { text: ')', kind: 'plain' },
  ]},
  { indent: 2, tokens: [
    { text: '.storage(', kind: 'plain' }, { text: 'Storage::', kind: 'fn' }, { text: 'Local', kind: 'accent' }, { text: ');', kind: 'punct' },
  ]},
  { tokens: [{ text: '}', kind: 'punct' }] },
];

const TOTAL = SCRIPT.reduce(
  (s, l) => s + (l.indent ?? 0) * 4 + l.tokens.reduce((a, t) => a + t.text.length, 0),
  0
);

function typed(chars: number): Line[] {
  let r = chars;
  const out: Line[] = [];
  for (const line of SCRIPT) {
    const ind = (line.indent ?? 0) * 4;
    if (r <= ind) { out.push({ indent: Math.floor(r / 4), tokens: [] }); return out; }
    r -= ind;
    const nl: Line = { indent: line.indent, tokens: [] };
    for (const tok of line.tokens) {
      if (r <= 0) { out.push(nl); return out; }
      if (r >= tok.text.length) { nl.tokens.push(tok); r -= tok.text.length; }
      else { nl.tokens.push({ ...tok, text: tok.text.slice(0, r) }); r = 0; break; }
    }
    out.push(nl);
  }
  return out;
}

const TOK_COLORS: Record<NonNullable<Tok['kind']>, string> = {
  comment: 'var(--syntax-comment)',
  keyword: 'var(--syntax-keyword)',
  string:  'var(--syntax-string)',
  accent:  'var(--syntax-accent)',
  fn:      'var(--syntax-fg)',
  punct:   'var(--syntax-muted)',
  plain:   'var(--syntax-fg)',
};

export default function HeroDemo() {
  const reduce = useReducedMotion();
  const [chars, setChars] = useState<number>(reduce ? TOTAL : 0);
  const [blink, setBlink] = useState<boolean>(true);
  const startedRef = useRef<boolean>(false);

  useEffect(() => {
    if (reduce) { setChars(TOTAL); return; }
    if (startedRef.current) return;
    startedRef.current = true;
    let raf = 0; let cancelled = false;
    const start = performance.now();
    const dur = 3800;
    const tick = (now: number) => {
      if (cancelled) return;
      const p = Math.min(1, (now - start) / dur);
      const e = 1 - Math.pow(1 - p, 2);
      setChars(Math.round(e * TOTAL));
      if (p < 1) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => { cancelled = true; cancelAnimationFrame(raf); };
  }, [reduce]);

  useEffect(() => {
    if (reduce) return;
    if (chars < TOTAL) return;
    const id = window.setInterval(() => setBlink((b) => !b), 540);
    return () => window.clearInterval(id);
  }, [chars, reduce]);

  const lines = typed(chars);
  const done = chars >= TOTAL;

  return (
    <motion.div
      className="hd"
      initial={reduce ? false : { opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.32, ease: [0.33, 1, 0.68, 1] }}
      aria-label="Writ editor preview"
    >
      <div className="hd-chrome">
        <div className="hd-traffic" aria-hidden="true">
          <span style={{ background: 'var(--traffic-close)' }} />
          <span style={{ background: 'var(--traffic-min)' }} />
          <span style={{ background: 'var(--traffic-max)' }} />
        </div>
        <div className="hd-tabs" aria-hidden="true">
          {[
            { name: 'scratch.rs', active: true },
            { name: 'todo.md', active: false },
            { name: 'release-notes.md', active: false },
          ].map((t) => (
            <span key={t.name} className={t.active ? 'hd-tab is-active' : 'hd-tab'}>
              {t.name}
            </span>
          ))}
        </div>
        <div className="hd-keys" aria-hidden="true">
          <kbd>⌘</kbd><kbd>⇧</kbd><kbd>Space</kbd>
        </div>
      </div>

      <pre className="hd-pre">
        <code>
          {lines.map((line, i) => (
            <span key={i} className="hd-row">
              <span className="hd-gutter">{i + 1}</span>
              <span className="hd-line">
                {' '.repeat((line.indent ?? 0) * 4)}
                {line.tokens.map((t, j) => (
                  <span key={j} style={{ color: TOK_COLORS[t.kind ?? 'plain'] }}>
                    {t.text}
                  </span>
                ))}
                {i === lines.length - 1 && (
                  <span
                    aria-hidden="true"
                    className="hd-caret"
                    style={{ opacity: done ? (blink ? 1 : 0) : 1 }}
                  />
                )}
              </span>
            </span>
          ))}
        </code>
      </pre>

      <div className="hd-status">
        <span className="hd-dot" aria-hidden="true" />
        <span>autosaved · local only</span>
        <span className="hd-status-right">
          <kbd>⌘</kbd><kbd>K</kbd>
          <span>command palette</span>
        </span>
      </div>
    </motion.div>
  );
}
