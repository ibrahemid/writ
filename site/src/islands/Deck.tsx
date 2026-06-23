import {
  useRef,
  useState,
  useEffect,
} from 'react';
import {
  useReducedMotion,
  useScroll,
  useSpring,
  useTransform,
  useMotionValueEvent,
  motion,
} from 'motion/react';

// ---------------------------------------------------------------------------
// SCENES — ported verbatim from mockup index.html:616-672
// ---------------------------------------------------------------------------
interface Scene {
  num: string;
  tab: string;
  title: string;
  lede: string;
  facts: [string, string][];
  status: string;
  body: string;
}

const SCENES: Scene[] = [
  {
    num: '01',
    tab: 'stdin → report',
    title: "Terminals can't render. Writ can.",
    lede: 'Pipe it, watch a folder, or set Writ as your default <code>.md</code> app. The raw stream becomes a real document, offline.',
    facts: [
      ['Pipe', '<span class="mono">claude -p "…" | writ -</span>'],
      ['Open', '<span class="mono">writ ~/agent-out</span>'],
      ['Watch', 'auto-render new files <span class="mut">(Settings)</span>'],
      ['Renders', '<span class="ar">md · Mermaid · KaTeX · HTML</span>'],
    ],
    status: '<span class="save"><i></i>Rendered</span><span class="sp"></span><span>offline</span>',
    body: `<div class="split">
      <div class="pane src"><div class="code"><span class="ln"><span class="c">$ claude -p "build" | writ -</span></span><span class="ln"><span class="hd"># Build status</span></span><span class="ln"></span><span class="ln">3 of 4 suites green.</span><span class="ln"></span><span class="ln"><span class="k">\`\`\`mermaid</span></span><span class="ln">graph TD A--&gt;B--&gt;C</span><span class="ln"><span class="k">\`\`\`</span></span></div></div>
      <div class="pane rnd"><div class="doc"><span class="tag">piped · rendered</span><h2>Build status</h2><p>3 of 4 suites green.</p>
        <div class="mermaid"><svg viewBox="0 0 120 150" width="78" height="116" role="img" aria-label="A to B to C">
          <g class="edge"><path d="M60 44 V64"/></g><g class="edge"><path d="M60 100 V120"/></g><polygon class="arrow" points="60,64 56,57 64,57"/><polygon class="arrow" points="60,120 56,113 64,113"/>
          <g class="node"><rect x="36" y="10" width="48" height="34" rx="8"/><text x="60" y="31" text-anchor="middle">A</text></g>
          <g class="node"><rect x="36" y="66" width="48" height="34" rx="8"/><text x="60" y="87" text-anchor="middle">B</text></g>
          <g class="node alt"><rect x="36" y="122" width="48" height="34" rx="8"/><text x="60" y="143" text-anchor="middle">C</text></g></svg></div></div></div>
    </div>`,
  },
  {
    num: '02',
    tab: 'FTS5 · every note',
    title: 'Dump everything. Find anything.',
    lede: 'The scratchpad that never asks you to name a file. Saved to plain text on disk, indexed the moment you stop typing.',
    facts: [
      ['autosave', 'every keystroke, to your folders'],
      ['search', '<span class="ar">SQLite FTS across every buffer</span>'],
      ['plain text', 'no app-locked store'],
    ],
    status: '<span>4 matches</span><span class="sp"></span><span>0.6 ms</span>',
    body: `<div class="search-in"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="M21 21l-4-4"/></svg>retention</div>
      <div class="sr on"><span class="file">kafka.yaml</span><span class="hit"><b>retention</b>.ms: 604800000</span><span class="n">L3</span></div>
      <div class="sr"><span class="file">migration.sql</span><span class="hit">bump <b>retention</b> to 7d</span><span class="n">L11</span></div>
      <div class="sr"><span class="file">scratch · 6d</span><span class="hit">log <b>retention</b> notes</span><span class="n">L7</span></div>
      <div class="sr"><span class="file">retro-q2.md</span><span class="hit"><b>retention</b> policy review</span><span class="n">L18</span></div>`,
  },
  {
    num: '03',
    tab: '10mb-export.json',
    title: 'Open anything. No spinner.',
    lede: 'Files up to about 30 MB open instantly; larger ones open gracefully, never a silent hang while other editors are still parsing.',
    facts: [
      ['≤ 30 MB', '<span class="ar">opens instantly</span>'],
      ['larger', 'opens gracefully, no silent hang'],
    ],
    status: '<span>10 MB · word-wrap on</span><span class="sp"></span><span>measured #117106</span>',
    body: `<div class="race">
      <div class="lane w"><span class="who">Writ</span><div class="tk"><div class="fl">opening…</div></div><span class="tm">instant</span></div>
      <div class="lane s"><span class="who">Sublime</span><div class="tk"><div class="fl"></div></div><span class="tm">fast</span></div>
      <div class="lane c"><span class="who">VS Code</span><div class="tk"><div class="fl" style="padding-right:11px">spinner…</div></div><span class="tm">75 s</span></div>
    </div>`,
  },
  {
    num: '04',
    tab: 'review.prompt',
    title: 'The prompt side of the loop.',
    lede: 'Draft prompts where you read the output. Live token count, reusable variables, and copy-as-prompt straight back to your agent.',
    facts: [
      ['{{vars}}', 'reusable templates, filled inline'],
      ['copy', '<span class="ar">one keypress to the terminal</span>'],
    ],
    status: '<span>≈ 38 tokens</span><span>·</span><span>3 variables</span>',
    body: `<div class="pw"><span class="cm"># review.prompt</span>
Review the diff in <span class="v">{{file}}</span> for <span class="v">{{concern}}</span>.

Output a numbered list. Be terse.
Flag anything that breaks <span class="v">{{invariant}}</span>.</div>
      <div class="pwbar"><span>≈ 38 tokens</span><span>·</span><span>3 variables</span><span class="cp">copy as prompt</span></div>`,
  },
  {
    num: '05',
    tab: 'untrusted.html',
    title: 'Safe to open whatever it wrote.',
    lede: 'Rendered HTML runs under a fixed content-security-policy in an isolated origin, making no outbound calls. What your agent wrote stays on your disk.',
    facts: [
      ['preview', 'no network <span class="mut">·</span> CSP default-src none'],
      ['no account', '<span class="ar">nothing to sign into</span>'],
    ],
    status: '<span>writ-preview://</span><span class="sp"></span><span style="color:var(--ok)">● 0 connections</span>',
    body: `<div class="safe">
      <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="var(--ok)" stroke-width="1.7" aria-hidden="true"><path d="M12 2 4 5v6c0 5 3.4 8.6 8 10 4.6-1.4 8-5 8-10V5Z"/><path d="m9 12 2 2 4-4"/></svg>
      <div class="big">No network</div>
      <div class="sub">preview origin · CSP default-src 'none'</div>
    </div>`,
  },
];

const N = SCENES.length;
const LABELS = ['Render', 'Search', 'Open', 'Prompt', 'Safe'];

// ---------------------------------------------------------------------------
// Static stacked fallback (reduced-motion OR ≤880px)
// ---------------------------------------------------------------------------
function StackedDeck() {
  return (
    <>
      {SCENES.map((scene) => (
        <div className="rcard" key={scene.num}>
          <div className="copy">
            <div className="dnum">{scene.num}</div>
            <h3 className="disp">{scene.title}</h3>
            <p className="lede2" dangerouslySetInnerHTML={{ __html: scene.lede }} />
            <div className="facts">
              {scene.facts.map(([label, value]) => (
                <div key={label}>
                  <b>{label}</b>
                  <span dangerouslySetInnerHTML={{ __html: value }} />
                </div>
              ))}
            </div>
          </div>
          <div>
            <div className="dwin">
              <div className="win-bar">
                <div className="lights" aria-hidden="true">
                  <i className="c" /><i className="m" /><i className="x" />
                </div>
                <div className="win-tabs">
                  <span className="tab active">
                    <span className="dot" />
                    {scene.tab}
                  </span>
                </div>
              </div>
              <div className="dbody" dangerouslySetInnerHTML={{ __html: scene.body }} />
              <div className="win-status" dangerouslySetInnerHTML={{ __html: scene.status }} />
            </div>
          </div>
        </div>
      ))}
    </>
  );
}

// ---------------------------------------------------------------------------
// Per-copy layer with motion transforms
// ---------------------------------------------------------------------------
function CopyLayer({
  scene,
  index,
  smoothProgress,
}: {
  scene: Scene;
  index: number;
  smoothProgress: ReturnType<typeof useSpring>;
}) {
  const opacity = useTransform(smoothProgress, (p: number) => {
    const ad = Math.abs(p * (N - 1) - index);
    return Math.max(0, Math.min(1, 1 - ad / 0.6));
  });

  const y = useTransform(smoothProgress, (p: number) => {
    const d = p * (N - 1) - index;
    return d * -30;
  });

  const pointerEvents = useTransform(smoothProgress, (p: number) => {
    const ad = Math.abs(p * (N - 1) - index);
    return (ad < 0.5 ? 'auto' : 'none') as 'auto' | 'none';
  });

  return (
    <motion.div
      className="copy clayer"
      data-i={index}
      style={{ opacity, translateY: y, pointerEvents }}
    >
      <div className="copyinner">
        <div className="dnum">{scene.num}</div>
        <h3 className="disp">{scene.title}</h3>
        <p className="lede2" dangerouslySetInnerHTML={{ __html: scene.lede }} />
        <div className="facts">
          {scene.facts.map(([label, value]) => (
            <div key={label}>
              <b>{label}</b>
              <span dangerouslySetInnerHTML={{ __html: value }} />
            </div>
          ))}
        </div>
      </div>
    </motion.div>
  );
}

// ---------------------------------------------------------------------------
// Per-body layer with motion transforms
// ---------------------------------------------------------------------------
function BodyLayer({
  scene,
  index,
  smoothProgress,
}: {
  scene: Scene;
  index: number;
  smoothProgress: ReturnType<typeof useSpring>;
}) {
  const opacity = useTransform(smoothProgress, (p: number) => {
    const ad = Math.abs(p * (N - 1) - index);
    return Math.max(0, Math.min(1, 1 - ad / 0.5));
  });

  const y = useTransform(smoothProgress, (p: number) => {
    const d = p * (N - 1) - index;
    return d * -14;
  });

  const scale = useTransform(smoothProgress, (p: number) => {
    const ad = Math.abs(p * (N - 1) - index);
    return 1 - Math.min(ad, 1) * 0.04;
  });

  // Fix 5: blur filter matching mockup line 718: ad>0.5 → blur(min((ad-0.5)*6,4)px)
  const filter = useTransform(smoothProgress, (p: number) => {
    const ad = Math.abs(p * (N - 1) - index);
    if (ad > 0.5) {
      return `blur(${Math.min((ad - 0.5) * 6, 4).toFixed(1)}px)`;
    }
    return 'none';
  });

  return (
    <motion.div
      className="clayer"
      data-i={index}
      style={{ opacity, translateY: y, scale, filter }}
    >
      <div dangerouslySetInnerHTML={{ __html: scene.body }} />
    </motion.div>
  );
}

// ---------------------------------------------------------------------------
// Motion deck — pinned scroll-morphing path
// ---------------------------------------------------------------------------
function MotionDeck() {
  const trackRef = useRef<HTMLDivElement>(null);
  const scrubRef = useRef<HTMLElement>(null);

  const { scrollYProgress } = useScroll({
    target: trackRef,
    offset: ['start start', 'end end'],
  });

  const smoothProgress = useSpring(scrollYProgress, {
    stiffness: 80,
    damping: 22,
    restDelta: 0.0006,
  });

  const [activeIdx, setActiveIdx] = useState(0);
  const [fillPct, setFillPct] = useState(0);
  const [inDeck, setInDeck] = useState(false);

  useMotionValueEvent(smoothProgress, 'change', (p) => {
    const pos = p * (N - 1);
    const near = Math.round(Math.max(0, Math.min(N - 1, pos)));
    setActiveIdx(near);
    setFillPct(p * 100);

    if (trackRef.current) {
      const r = trackRef.current.getBoundingClientRect();
      const nowInDeck = r.top <= 1 && r.bottom >= window.innerHeight - 1;
      setInDeck(nowInDeck);
      // Imperatively toggle inert so off-deck buttons aren't keyboard tab-stops
      if (scrubRef.current) {
        (scrubRef.current as HTMLElement & { inert: boolean }).inert = !nowInDeck;
      }
    }
  });

  // Initialize scrubber as inert until first scroll event confirms we're in-deck
  useEffect(() => {
    if (scrubRef.current) {
      (scrubRef.current as HTMLElement & { inert: boolean }).inert = true;
    }
  }, []);

  // bg parallax
  const bgY = useTransform(smoothProgress, [0, 1], [0, -56]);

  // Fix 4: surface scale matching mockup line 721:
  // nf = Math.abs(pos - near) where pos = p*(N-1), near = Math.round(pos)
  // scale = 1 - nf*0.018
  const surfaceScale = useTransform(smoothProgress, (p: number) => {
    const pos = p * (N - 1);
    const near = Math.round(Math.max(0, Math.min(N - 1, pos)));
    const nf = Math.abs(pos - near);
    return 1 - nf * 0.018;
  });

  const surfaceY = useTransform(smoothProgress, (p: number) => {
    return (0.5 - Math.max(0, Math.min(1, p))) * 22;
  });

  function scrollToScene(i: number) {
    if (!trackRef.current) return;
    const r = trackRef.current.getBoundingClientRect().top + window.scrollY;
    const total = trackRef.current.offsetHeight - window.innerHeight;
    window.scrollTo({ top: Math.round(r + (i / (N - 1)) * total), behavior: 'smooth' });
  }

  return (
    <>
      <section className="deck" id="deck" aria-label="Capabilities">
        <div
          className="deck-track"
          id="track"
          ref={trackRef}
          style={{ height: `${N * 100}vh` }}
        >
          <div className="stage">
            <motion.div className="bg" id="bg" style={{ translateY: bgY }} />
            <div className="mode live" id="modeA">
              <div className="wrapA">
                {/* Copy column */}
                <div className="copycol">
                  {SCENES.map((scene, i) => (
                    <CopyLayer
                      key={scene.num}
                      scene={scene}
                      index={i}
                      smoothProgress={smoothProgress}
                    />
                  ))}
                </div>

                {/* Persistent surface window */}
                <motion.div className="surface" style={{ translateY: surfaceY, scale: surfaceScale }}>
                  <div className="dwin">
                    <div className="win-bar">
                      <div className="lights" aria-hidden="true">
                        <i className="c" /><i className="m" /><i className="x" />
                      </div>
                      <div className="win-tabs">
                        <span className="tab active">
                          <span className="dot" />
                          <span id="surfTabName">{SCENES[activeIdx]!.tab}</span>
                        </span>
                      </div>
                    </div>
                    <div className="dbody surf-body">
                      {SCENES.map((scene, i) => (
                        <BodyLayer
                          key={scene.num}
                          scene={scene}
                          index={i}
                          smoothProgress={smoothProgress}
                        />
                      ))}
                    </div>
                    <div
                      id="surfStatus"
                      className="win-status"
                      dangerouslySetInnerHTML={{ __html: SCENES[activeIdx]!.status }}
                    />
                  </div>
                </motion.div>
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Scrubber — fixed position via CSS; inert toggled imperatively when off-deck */}
      <nav
        ref={scrubRef}
        className={`scrub${inDeck ? ' show' : ''}`}
        id="scrub"
        aria-label="Deck scenes"
      >
        <div className="rail" aria-hidden="true">
          <div className="fill" style={{ height: `${fillPct.toFixed(2)}%` }} />
        </div>
        {SCENES.map((_, i) => (
          <button
            key={i}
            data-i={i}
            aria-label={`Scene ${i + 1}: ${LABELS[i]}`}
            className={i === activeIdx ? 'on' : undefined}
            onClick={() => scrollToScene(i)}
          >
            <span className="lab">{LABELS[i]}</span>
            <span className="pt" aria-hidden="true" />
          </button>
        ))}
      </nav>
    </>
  );
}

// ---------------------------------------------------------------------------
// Root Deck island — mode decided post-mount to avoid SSR mismatch
//
// Theme note: Deck does NOT listen for the 'writ:theme' event. All colors in
// the deck scenes and window chrome reference CSS custom properties
// (var(--bg), var(--line), var(--accent), var(--ok), etc.) that cascade from
// the data-theme attribute on <html>. The toggle in Header.astro sets
// data-theme directly, so the deck recolors automatically via CSS — no JS
// listener or re-render needed.
// ---------------------------------------------------------------------------
type DeckMode = 'static' | 'motion' | null;

export default function Deck() {
  // useReducedMotion is safe to call unconditionally (subscribes to media-query)
  const prefersReduced = useReducedMotion();
  const [mode, setMode] = useState<DeckMode>(null);

  useEffect(() => {
    const narrowMq = window.matchMedia('(max-width: 880px)');
    const reducedMq = window.matchMedia('(prefers-reduced-motion: reduce)');

    function evaluate() {
      const isNarrow = narrowMq.matches;
      const isReduced = !!prefersReduced || reducedMq.matches;
      setMode(isReduced || isNarrow ? 'static' : 'motion');
    }

    // Fix 3: react to resize crossing the 880px breakpoint
    narrowMq.addEventListener('change', evaluate);
    evaluate();

    return () => {
      narrowMq.removeEventListener('change', evaluate);
    };
  }, [prefersReduced]);

  // null = SSR / pre-hydration: render nothing
  if (mode === null) return null;

  if (mode === 'static') {
    return (
      <section className="deck stacked" id="deck" aria-label="Capabilities">
        <div className="deck-track" id="track">
          <div className="stage">
            <div className="mode" id="modeA">
              <StackedDeck />
            </div>
          </div>
        </div>
      </section>
    );
  }

  // Motion path: MotionDeck mounts here — scroll listeners registered only now
  return <MotionDeck />;
}
