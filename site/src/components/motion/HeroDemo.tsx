import { motion, useReducedMotion, type Variants } from 'framer-motion';
import { useEffect, useMemo, useState } from 'react';

type Token = {
  text: string;
  kind?: 'keyword' | 'string' | 'comment' | 'accent' | 'fn' | 'punct';
};

type Line = {
  indent?: number;
  tokens: Token[];
};

const SCRIPT: Line[] = [
  { tokens: [{ text: '// scratchpad — always a keypress away', kind: 'comment' }] },
  {
    tokens: [
      { text: 'const', kind: 'keyword' },
      { text: ' ' },
      { text: 'writ', kind: 'accent' },
      { text: ' = ' },
      { text: 'new', kind: 'keyword' },
      { text: ' ' },
      { text: 'Scratchpad', kind: 'fn' },
      { text: '({', kind: 'punct' },
    ],
  },
  {
    indent: 1,
    tokens: [
      { text: 'hotkey', kind: 'accent' },
      { text: ': ' },
      { text: "'Cmd+Shift+Space'", kind: 'string' },
      { text: ',', kind: 'punct' },
    ],
  },
  {
    indent: 1,
    tokens: [
      { text: 'autosave', kind: 'accent' },
      { text: ': ' },
      { text: 'true', kind: 'keyword' },
      { text: ',', kind: 'punct' },
    ],
  },
  {
    indent: 1,
    tokens: [
      { text: 'storage', kind: 'accent' },
      { text: ': ' },
      { text: "'local'", kind: 'string' },
      { text: ',', kind: 'punct' },
    ],
  },
  { tokens: [{ text: '})', kind: 'punct' }] },
];

const TOTAL_CHARS = SCRIPT.reduce(
  (sum, line) => sum + (line.indent ?? 0) * 2 + line.tokens.reduce((s, t) => s + t.text.length, 0),
  0,
);

function typedScript(chars: number) {
  let remaining = chars;
  const rendered: Line[] = [];
  for (const line of SCRIPT) {
    const indentChars = (line.indent ?? 0) * 2;
    if (remaining <= indentChars) {
      rendered.push({ indent: Math.floor(remaining / 2), tokens: [] });
      return rendered;
    }
    remaining -= indentChars;
    const newLine: Line = { indent: line.indent, tokens: [] };
    for (const token of line.tokens) {
      if (remaining <= 0) {
        rendered.push(newLine);
        return rendered;
      }
      if (remaining >= token.text.length) {
        newLine.tokens.push(token);
        remaining -= token.text.length;
      } else {
        newLine.tokens.push({ ...token, text: token.text.slice(0, remaining) });
        remaining = 0;
        break;
      }
    }
    rendered.push(newLine);
  }
  return rendered;
}

const cursorVariants: Variants = {
  on: { opacity: 1 },
  off: { opacity: 0 },
};

export default function HeroDemo() {
  const reduce = useReducedMotion();
  const [charCount, setCharCount] = useState(reduce ? TOTAL_CHARS : 0);

  useEffect(() => {
    if (reduce) {
      setCharCount(TOTAL_CHARS);
      return;
    }
    let frame = 0;
    let cancelled = false;
    const start = performance.now();
    const duration = 3600;

    function tick(now: number) {
      if (cancelled) return;
      const elapsed = now - start;
      const progress = Math.min(1, elapsed / duration);
      const eased = 1 - Math.pow(1 - progress, 2);
      setCharCount(Math.round(eased * TOTAL_CHARS));
      if (progress < 1) {
        frame = requestAnimationFrame(tick);
      }
    }

    frame = requestAnimationFrame(tick);
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
    };
  }, [reduce]);

  const lines = useMemo(() => typedScript(charCount), [charCount]);
  const done = charCount >= TOTAL_CHARS;

  return (
    <motion.div
      className="hero-demo"
      initial={reduce ? undefined : { opacity: 0, y: 12 }}
      animate={reduce ? undefined : { opacity: 1, y: 0 }}
      transition={{ duration: 0.5, ease: [0.33, 1, 0.68, 1] }}
    >
      <div className="hero-demo-chrome" aria-hidden="true">
        <div className="hero-demo-dots">
          <span></span>
          <span></span>
          <span></span>
        </div>
        <div className="hero-demo-tabs">
          <span className="hero-demo-tab is-active">scratch.ts</span>
          <span className="hero-demo-tab">todo.md</span>
          <span className="hero-demo-tab">release-notes.md</span>
        </div>
        <div className="hero-demo-hotkey">
          <kbd>⌘</kbd>
          <kbd>⇧</kbd>
          <kbd>Space</kbd>
        </div>
      </div>
      <pre className="hero-demo-code" aria-label="Writ editor preview">
        <code>
          {lines.map((line, lineIdx) => (
            <span className="hdl" key={lineIdx}>
              <span className="hdl-gutter">{lineIdx + 1}</span>
              <span className="hdl-body">
                {'  '.repeat(line.indent ?? 0)}
                {line.tokens.map((tok, tokIdx) => (
                  <span key={tokIdx} className={`tok tok-${tok.kind ?? 'plain'}`}>
                    {tok.text}
                  </span>
                ))}
                {lineIdx === lines.length - 1 && (
                  <motion.span
                    className="hdl-caret"
                    aria-hidden="true"
                    variants={cursorVariants}
                    initial="on"
                    animate={reduce ? 'on' : done ? ['on', 'off'] : 'on'}
                    transition={
                      reduce
                        ? undefined
                        : { duration: 0.55, repeat: done ? Infinity : 0, repeatType: 'reverse' }
                    }
                  />
                )}
              </span>
            </span>
          ))}
        </code>
      </pre>
      <div className="hero-demo-status">
        <span className="hds-dot" aria-hidden="true"></span>
        <span>autosaved · local only</span>
        <span className="hero-demo-status-right">
          <kbd>⇧</kbd>
          <kbd>⇧</kbd>
          <span>command palette</span>
        </span>
      </div>
    </motion.div>
  );
}
