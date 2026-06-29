import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from 'react';
import {
  BUFFERS,
  DEFAULT_CONTENTS,
  FMT,
  GROUPS,
  HISTORY,
  OPEN_FILES,
  type BufferMeta,
} from './writ/buffers';
import { escapeHtml, estimateTokens, formatTokens, mdToHtml } from './writ/render';
import { highlightCode } from './writ/highlight';
import { genHex } from './writ/hex';
import { applyTransform, type TransformId } from './writ/transforms';

interface Caret {
  ln: number;
  col: number;
}

interface SearchHit {
  id: string;
  line: number;
  text: string;
}

const TRANSFORM_IDS: TransformId[] = [
  'trim',
  'dedent',
  'finalnl',
  'prompt',
  'tidy',
  'normalize',
  'punct',
  'quotes',
];

function mermaidThemeVars(isDark: boolean) {
  return {
    primaryColor: isDark ? '#1a1a2e' : '#eef1f7',
    primaryBorderColor: isDark ? '#8aa6ff' : '#3b5bdb',
    primaryTextColor: isDark ? '#e0e0e0' : '#1a1a22',
    lineColor: '#8a8aa0',
    fontSize: '12px',
  };
}

const MONO = 'var(--font-mono)';

export default function WritWindow() {
  const dynamicBuffers = useRef<Record<string, BufferMeta>>({});
  const newCount = useRef(0);
  const hover = useRef(false);
  const lastShift = useRef(0);
  const lastMermaid = useRef('');
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const rootRef = useRef<HTMLDivElement>(null);
  const gutterRef = useRef<HTMLPreElement>(null);
  const previewRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const paletteRef = useRef<HTMLInputElement>(null);
  const renameRef = useRef<HTMLInputElement>(null);

  const [contents, setContents] = useState<Record<string, string>>(() => ({ ...DEFAULT_CONTENTS }));
  const [names, setNames] = useState<Record<string, string>>({});
  const [activeId, setActiveId] = useState('report.md');
  const [tabs, setTabs] = useState<string[]>(() => OPEN_FILES.slice());
  const [viewMode, setViewMode] = useState<'source' | 'split' | 'preview'>('split');
  const [query, setQuery] = useState('');
  const [searchMs, setSearchMs] = useState(0);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [paletteQuery, setPaletteQuery] = useState('');
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [zoom, setZoom] = useState(1);
  const [watching, setWatching] = useState(false);
  const [edited, setEdited] = useState(false);
  const [caret, setCaret] = useState<Caret>({ ln: 1, col: 1 });
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameTemp, setRenameTemp] = useState('');
  const [narrow, setNarrow] = useState(false);

  const meta = useCallback(
    (id: string): BufferMeta =>
      dynamicBuffers.current[id] ?? BUFFERS[id] ?? (BUFFERS['report.md'] as BufferMeta),
    [],
  );
  const nameOf = useCallback(
    (id: string): string => names[id] || meta(id)?.name || id,
    [names, meta],
  );
  const defaultView = useCallback(
    (id: string): 'source' | 'split' => {
      const lang = meta(id).lang;
      return lang === 'md' || lang === 'html' ? 'split' : 'source';
    },
    [meta],
  );

  const markEdited = useCallback(() => {
    setEdited(true);
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => setEdited(false), 1000);
  }, []);

  const scrollWin = useCallback(() => {
    const el = rootRef.current;
    if (!el || typeof window.scrollTo !== 'function') return;
    try {
      const y = el.getBoundingClientRect().top + window.scrollY - 64;
      window.scrollTo({ top: y, behavior: 'smooth' });
    } catch {
      /* scrollTo unavailable (e.g. jsdom) */
    }
  }, []);

  const open = useCallback(
    (id: string) => {
      setTabs((prev) => (prev.indexOf(id) < 0 ? [...prev, id] : prev));
      setActiveId(id);
      setViewMode(defaultView(id));
      setEdited(false);
      setCaret({ ln: 1, col: 1 });
    },
    [defaultView],
  );

  const closeTab = useCallback(
    (id: string) => {
      setTabs((prev) => {
        let next = prev.filter((x) => x !== id);
        if (next.length === 0) next = ['report.md'];
        setActiveId((active) => {
          if (active !== id) return active;
          const fallback = next[next.length - 1] ?? 'report.md';
          setViewMode(defaultView(fallback));
          return fallback;
        });
        return next;
      });
    },
    [defaultView],
  );

  const updateCaret = useCallback((ta: HTMLTextAreaElement) => {
    const p = ta.selectionStart || 0;
    const before = ta.value.slice(0, p);
    const ln = before.split('\n').length;
    const col = p - before.lastIndexOf('\n');
    setCaret({ ln, col });
  }, []);

  const onEdit = useCallback(
    (e: React.ChangeEvent<HTMLTextAreaElement>) => {
      const value = e.target.value;
      setContents((prev) => ({ ...prev, [activeId]: value }));
      updateCaret(e.target);
      markEdited();
    },
    [activeId, markEdited, updateCaret],
  );

  const computeResults = useCallback(
    (q: string): SearchHit[] => {
      const query = q.trim();
      if (!query) return [];
      const lc = query.toLowerCase();
      const res: SearchHit[] = [];
      for (const [id, b] of Object.entries(BUFFERS)) {
        if (b.lang === 'binary') continue;
        const lines = (contents[id] || '').split('\n');
        for (let i = 0; i < lines.length; i++) {
          const text = lines[i] ?? '';
          if (text.toLowerCase().indexOf(lc) >= 0) {
            res.push({ id, line: i + 1, text });
            break;
          }
        }
      }
      return res;
    },
    [contents],
  );

  const runSearch = useCallback(
    (q: string) => {
      const t0 = performance.now();
      computeResults(q);
      const t1 = performance.now();
      setQuery(q);
      setSearchMs(Math.max(0.1, Math.round((t1 - t0) * 10) / 10));
    },
    [computeResults],
  );

  const setView = useCallback((v: 'source' | 'split' | 'preview') => setViewMode(v), []);

  const cmdNewTab = useCallback(() => {
    newCount.current += 1;
    const id = 'scratch-' + newCount.current;
    dynamicBuffers.current[id] = {
      name: 'untitled-' + newCount.current + '.md',
      lang: 'md',
      label: 'Markdown',
      dot: 'var(--accent)',
    };
    setContents((prev) => ({ ...prev, [id]: '' }));
    setTabs((prev) => [...prev, id]);
    setActiveId(id);
    setViewMode('split');
    setPaletteOpen(false);
    setEdited(false);
    setCaret({ ln: 1, col: 1 });
  }, []);

  const runCmd = useCallback(
    (id: string) => {
      if (TRANSFORM_IDS.includes(id as TransformId)) {
        setContents((prev) => ({
          ...prev,
          [activeId]: applyTransform(id as TransformId, prev[activeId] || ''),
        }));
        setPaletteOpen(false);
        markEdited();
        return;
      }
      if (id === 'newtab') return cmdNewTab();
      if (id === 'closetab') {
        setPaletteOpen(false);
        return closeTab(activeId);
      }
      if (id === 'switchtab') {
        setPaletteOpen(false);
        const i = tabs.indexOf(activeId);
        return open(tabs[(i + 1) % tabs.length] ?? activeId);
      }
      if (id === 'renametab') {
        setPaletteOpen(false);
        setRenamingId(activeId);
        setRenameTemp(nameOf(activeId));
        setTimeout(() => {
          renameRef.current?.focus();
          renameRef.current?.select();
        }, 40);
        return;
      }
      if (id === 'togglesidebar') {
        setSidebarOpen((s) => !s);
        setPaletteOpen(false);
        return;
      }
      if (id === 'find' || id === 'search') {
        setSidebarOpen(true);
        setPaletteOpen(false);
        setTimeout(() => searchRef.current?.focus(), 50);
        return;
      }
      if (id === 'zoomin') {
        setZoom((z) => Math.min(1.6, Math.round((z + 0.1) * 10) / 10));
        setPaletteOpen(false);
        return;
      }
      if (id === 'zoomout') {
        setZoom((z) => Math.max(0.7, Math.round((z - 0.1) * 10) / 10));
        setPaletteOpen(false);
        return;
      }
      if (id === 'zoomreset') {
        setZoom(1);
        setPaletteOpen(false);
        return;
      }
      if (id === 'watchinbox') {
        setWatching((w) => !w);
        setSidebarOpen(true);
        setPaletteOpen(false);
        return;
      }
      if (id === 'copyprompt') {
        const out = applyTransform('prompt', contents[activeId] || '');
        try {
          void navigator.clipboard?.writeText(out);
        } catch {
          /* clipboard unavailable */
        }
        setEdited(false);
        setPaletteOpen(false);
        return;
      }
    },
    [activeId, tabs, contents, nameOf, cmdNewTab, closeTab, open, markEdited],
  );

  const commitRename = useCallback(() => {
    setRenamingId((id) => {
      if (!id) return null;
      const nm = renameTemp.trim() || nameOf(id);
      setNames((prev) => ({ ...prev, [id]: nm }));
      return null;
    });
  }, [renameTemp, nameOf]);

  const openPalette = useCallback(() => {
    setPaletteOpen(true);
    setPaletteQuery('');
    scrollWin();
    setTimeout(() => paletteRef.current?.focus(), 40);
  }, [scrollWin]);

  const togglePalette = useCallback(() => {
    setPaletteOpen((o) => {
      const next = !o;
      if (next) {
        setPaletteQuery('');
        setTimeout(() => paletteRef.current?.focus(), 30);
      }
      return next;
    });
  }, []);

  const currentPaletteCmds = useMemo(() => {
    const pq = paletteQuery.trim().toLowerCase();
    const out: { id: string; name: string; desc: string; kbd: string }[] = [];
    for (const g of GROUPS) {
      for (const c of g.cmds) {
        if (!pq || (c.name + ' ' + c.desc).toLowerCase().includes(pq)) {
          out.push({ id: c.id, name: c.name, desc: c.desc, kbd: c.kbd || '' });
        }
      }
    }
    return out;
  }, [paletteQuery]);

  const paletteItems = useMemo(() => {
    const pq = paletteQuery.trim().toLowerCase();
    const items: { kind: 'header' | 'cmd'; key: string; label?: string; cmd?: typeof GROUPS[number]['cmds'][number] }[] = [];
    for (const g of GROUPS) {
      const matched = g.cmds.filter(
        (c) => !pq || (c.name + ' ' + c.desc).toLowerCase().includes(pq),
      );
      if (matched.length) {
        items.push({ kind: 'header', key: 'h-' + g.label, label: g.label });
        for (const c of matched) items.push({ kind: 'cmd', key: c.id, cmd: c });
      }
    }
    return items;
  }, [paletteQuery]);

  const loadFmt = useCallback(
    (k: 'md' | 'html' | 'mermaid' | 'math') => {
      open(FMT[k]);
      scrollWin();
    },
    [open, scrollWin],
  );

  useEffect(() => {
    const onResize = () => setNarrow(window.innerWidth < 720);
    onResize();
    window.addEventListener('resize', onResize);

    const onKey = (e: KeyboardEvent) => {
      const k = e.key;
      // Command palette opens on a double-tap of Shift (matches the app's Shift+Shift binding).
      if (k === 'Shift' && !e.repeat && !e.metaKey && !e.ctrlKey && !e.altKey) {
        const now = performance.now();
        const within = now - lastShift.current < 400;
        lastShift.current = now;
        if (within) {
          const focusWithin = rootRef.current?.contains(document.activeElement);
          if (hover.current || focusWithin || paletteOpen) {
            e.preventDefault();
            lastShift.current = 0;
            togglePalette();
          }
        }
        return;
      }
      lastShift.current = 0;
      if (k === 'Escape' && paletteOpen) {
        setPaletteOpen(false);
      }
    };
    document.addEventListener('keydown', onKey);

    const onCmd = (e: Event) => {
      const detail = (e as CustomEvent).detail as { action: string; arg?: string };
      if (!detail) return;
      if (detail.action === 'loadFmt') loadFmt(detail.arg as 'md' | 'html' | 'mermaid' | 'math');
      else if (detail.action === 'open' && detail.arg) {
        open(detail.arg);
        scrollWin();
      } else if (detail.action === 'palette') openPalette();
      else if (detail.action === 'demoRender') {
        open('report.md');
        setViewMode('split');
        setQuery('');
        scrollWin();
      } else if (detail.action === 'search') {
        setSidebarOpen(true);
        runSearch(detail.arg || 'settle');
        scrollWin();
        setTimeout(() => searchRef.current?.focus(), 60);
      }
    };
    document.addEventListener('writ:cmd', onCmd);

    return () => {
      window.removeEventListener('resize', onResize);
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('writ:cmd', onCmd);
      if (saveTimer.current) clearTimeout(saveTimer.current);
    };
  }, [paletteOpen, togglePalette, loadFmt, open, openPalette, runSearch, scrollWin]);

  useEffect(() => {
    let cancelled = false;
    const el = previewRef.current;
    if (!el) return;
    const isDark = document.documentElement.getAttribute('data-theme') === 'dark';

    async function renderLibs() {
      if (!el) return;
      if (el.querySelector('.math, [data-src], pre.mermaid') || el.textContent?.includes('$')) {
        try {
          const [{ default: renderMathInElement }] = await Promise.all([
            import('katex/dist/contrib/auto-render'),
            import('katex/dist/katex.min.css'),
          ]);
          if (cancelled) return;
          renderMathInElement(el, {
            delimiters: [
              { left: '$$', right: '$$', display: true },
              { left: '$', right: '$', display: false },
            ],
            throwOnError: false,
          });
        } catch {
          /* katex optional */
        }
      }
      const nodes = Array.from(el.querySelectorAll<HTMLElement>('pre.mermaid'));
      if (nodes.length) {
        const sig =
          (isDark ? 'd' : 'l') + '|' + nodes.map((n) => n.getAttribute('data-src') || '').join('~~');
        if (sig === lastMermaid.current) return;
        lastMermaid.current = sig;
        try {
          const { default: mermaid } = await import('mermaid');
          if (cancelled) return;
          mermaid.initialize({
            startOnLoad: false,
            securityLevel: 'strict',
            theme: 'base',
            fontFamily: 'Inter, sans-serif',
            themeVariables: mermaidThemeVars(isDark),
          });
          nodes.forEach((n) => {
            n.removeAttribute('data-processed');
            n.innerHTML = escapeHtml(n.getAttribute('data-src') || n.textContent || '');
          });
          await mermaid.run({ nodes, suppressErrors: true });
        } catch {
          /* mermaid optional */
        }
      }
    }
    void renderLibs();

    const onTheme = () => {
      lastMermaid.current = '';
      void renderLibs();
    };
    document.addEventListener('writ:theme', onTheme);
    return () => {
      cancelled = true;
      document.removeEventListener('writ:theme', onTheme);
    };
  }, [contents, activeId, viewMode]);

  const b = meta(activeId);
  const lang = b.lang;
  const content = contents[activeId] || '';
  const hasPreview = lang === 'md' || lang === 'html';
  const tokenLabel = b.tok ? b.tok : formatTokens(estimateTokens(content));
  const results = computeResults(query);
  const showResults = query.trim().length > 0;

  const bodyCols = sidebarOpen ? '232px minmax(0,1fr)' : 'minmax(0,1fr)';

  return (
    <div ref={rootRef} className="ww-root">
      <div
        className="ww-window"
        onMouseEnter={() => (hover.current = true)}
        onMouseLeave={() => (hover.current = false)}
      >
        <div className="ww-topbar">
          <div className="ww-lights" aria-hidden="true">
            <span style={{ background: '#ff5f57' }} />
            <span style={{ background: '#febc2e' }} />
            <span style={{ background: '#28c840' }} />
          </div>
          <div className="ww-tabstrip sidescroll">
            {tabs.map((tid) => (
              <div
                key={tid}
                className="ftab"
                role="button"
                tabIndex={0}
                aria-pressed={tid === activeId}
                data-active={tid === activeId}
                onClick={() => open(tid)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    open(tid);
                  }
                }}
                title={nameOf(tid)}
              >
                <span
                  className="ww-dot"
                  style={{ background: meta(tid).dot }}
                  aria-hidden="true"
                />
                {renamingId === tid ? (
                  <input
                    ref={renameRef}
                    value={renameTemp}
                    onChange={(e) => setRenameTemp(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') commitRename();
                      else if (e.key === 'Escape') setRenamingId(null);
                    }}
                    onBlur={commitRename}
                    onClick={(e) => e.stopPropagation()}
                    aria-label="Rename buffer"
                    className="ww-rename"
                  />
                ) : (
                  <span>{nameOf(tid)}</span>
                )}
                <span
                  className="x"
                  role="button"
                  tabIndex={0}
                  aria-label={`Close ${nameOf(tid)}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    closeTab(tid);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      e.stopPropagation();
                      closeTab(tid);
                    }
                  }}
                >
                  ×
                </span>
              </div>
            ))}
            <button className="ww-newtab" onClick={cmdNewTab} title="New tab" aria-label="New tab" type="button">
              +
            </button>
          </div>
          <button className="ww-kbtn hide-sm" onClick={openPalette} aria-label="Open command palette (Shift Shift)" type="button">
            ⇧⇧
          </button>
        </div>

        <div className="ww-body" style={{ gridTemplateColumns: bodyCols }}>
          {sidebarOpen && (
            <aside className="ww-side win-side">
              <div className="ww-search-wrap">
                <div className="ww-search">
                  <span aria-hidden="true" className="ww-search-icon">
                    ⌕
                  </span>
                  <input
                    ref={searchRef}
                    value={query}
                    onChange={(e) => runSearch(e.target.value)}
                    placeholder="Search buffers…"
                    aria-label="Search buffers"
                    autoComplete="off"
                    spellCheck={false}
                    className="ww-search-input"
                  />
                  {showResults && (
                    <span className="ww-search-count" aria-live="polite">
                      {results.length + (results.length === 1 ? ' hit' : ' hits')}
                    </span>
                  )}
                </div>
              </div>
              <div className="ww-side-scroll sidescroll">
                {showResults ? (
                  <div>
                    <div className="ww-side-h">RESULTS</div>
                    {results.map((r) => (
                      <button
                        key={r.id}
                        className="srow"
                        data-active={r.id === activeId}
                        onClick={() => open(r.id)}
                      >
                        <div className="ww-srow-top">
                          <span className="ww-srow-name">{nameOf(r.id)}</span>
                          <span className="ww-srow-loc">L{r.line}</span>
                        </div>
                        <div
                          className="ww-srow-line"
                          dangerouslySetInnerHTML={{ __html: highlightHit(r.text.trim(), query) }}
                        />
                      </button>
                    ))}
                    <div className="ww-side-foot">
                      {results.length + ' file' + (results.length === 1 ? '' : 's')} · {searchMs} ms
                    </div>
                  </div>
                ) : (
                  <div>
                    <div className="ww-side-row-h">
                      <span className="ww-side-h">OPEN</span>
                      {watching && (
                        <span className="ww-watching">
                          <span className="ww-watch-dot" />
                          watching inbox
                        </span>
                      )}
                    </div>
                    {OPEN_FILES.map((fid) => (
                      <button
                        key={fid}
                        className="srow"
                        data-active={fid === activeId}
                        onClick={() => open(fid)}
                      >
                        <span
                          className="ww-open-name"
                          style={{ color: fid === activeId ? 'var(--accent)' : 'var(--foreground)' }}
                        >
                          {nameOf(fid)}
                        </span>
                      </button>
                    ))}
                    <div className="ww-side-h ww-side-h-spaced">HISTORY · TODAY</div>
                    {HISTORY.map((h) => (
                      <button
                        key={h.id}
                        className="srow ww-hist"
                        data-active={h.id === activeId}
                        onClick={() => open(h.id)}
                      >
                        <span className="ww-hist-name">{nameOf(h.id)}</span>
                        <span className="ww-hist-when">{h.when}</span>
                      </button>
                    ))}
                  </div>
                )}
              </div>
            </aside>
          )}

          <div className="ww-main">
            <MainPane
              lang={lang}
              content={content}
              viewMode={viewMode}
              narrow={narrow}
              zoom={zoom}
              gutterRef={gutterRef}
              previewRef={previewRef}
              onEdit={onEdit}
              onCaret={(e) => updateCaret(e.currentTarget)}
              onGutterSync={(e) => {
                if (gutterRef.current) gutterRef.current.scrollTop = e.currentTarget.scrollTop;
              }}
            />
          </div>

          {paletteOpen && (
            <div className="ww-palette-scrim" onClick={() => setPaletteOpen(false)}>
              <div className="ww-palette" onClick={(e) => e.stopPropagation()}>
                <div className="ww-palette-head">
                  <span className="ww-palette-glyph">⇧⇧</span>
                  <input
                    ref={paletteRef}
                    value={paletteQuery}
                    onChange={(e) => setPaletteQuery(e.target.value)}
                    onKeyDown={(e) => {
                      const first = currentPaletteCmds[0];
                      if (e.key === 'Enter' && first) {
                        e.preventDefault();
                        runCmd(first.id);
                      }
                    }}
                    placeholder="Type a command…"
                    aria-label="Command palette"
                    autoComplete="off"
                    spellCheck={false}
                    className="ww-palette-input"
                  />
                </div>
                <div className="ww-palette-list sidescroll">
                  {paletteItems.length === 0 && (
                    <div className="ww-palette-empty">No matching command.</div>
                  )}
                  {paletteItems.map((it) =>
                    it.kind === 'header' ? (
                      <div key={it.key} className="ww-palette-group">
                        {it.label}
                      </div>
                    ) : (
                      <button key={it.key} className="pcmd" onClick={() => runCmd(it.cmd!.id)}>
                        <span className="ww-pcmd-text">
                          <span className="ww-pcmd-name">{it.cmd!.name}</span>
                          <span className="ww-pcmd-desc">{it.cmd!.desc}</span>
                        </span>
                        {it.cmd!.kbd && <span className="ww-pcmd-kbd">{it.cmd!.kbd}</span>}
                      </button>
                    ),
                  )}
                </div>
              </div>
            </div>
          )}
        </div>

        <div className="ww-status">
          <span style={{ color: edited ? 'var(--warn)' : 'var(--success)' }}>
            {edited ? 'Edited' : 'Saved'}
          </span>
          <span style={{ color: 'var(--subtle)' }}>
            Ln {caret.ln}, Col {caret.col}
          </span>
          {b.badge && <span style={{ color: 'var(--warn)' }}>{b.badge}</span>}
          <span className="hide-sm">{b.label}</span>
          <span className="hide-sm" style={{ color: 'var(--subtle)' }}>
            UTF-8
          </span>
          <span style={{ color: 'var(--accent)' }}>≈ {tokenLabel} tok</span>
          {zoom !== 1 && (
            <span style={{ color: 'var(--subtle)' }}>{Math.round(zoom * 100)}%</span>
          )}
          <div style={{ flex: 1 }} />
          {hasPreview && (
            <div className="ww-viewtoggle">
              <button className="stog" aria-pressed={viewMode === 'source'} onClick={() => setView('source')}>
                Source
              </button>
              <button className="stog" aria-pressed={viewMode === 'split'} onClick={() => setView('split')}>
                Split
              </button>
              <button className="stog" aria-pressed={viewMode === 'preview'} onClick={() => setView('preview')}>
                Preview
              </button>
            </div>
          )}
          <button className="stog" onClick={openPalette} aria-label="Open command palette (Shift Shift)" style={{ color: 'var(--foreground)' }}>
            ⇧⇧
          </button>
        </div>
      </div>
      <p className="ww-caption">
        Live window. Switch tabs, edit <span className="ww-mono">report.md</span>, double-tap{' '}
        <span className="ww-mono">⇧</span>, search.
      </p>
    </div>
  );
}

function highlightHit(text: string, q: string): string {
  const query = q.trim();
  if (!query) return escapeHtml(text);
  const idx = text.toLowerCase().indexOf(query.toLowerCase());
  if (idx < 0) return escapeHtml(text.length > 60 ? text.slice(0, 60) + '…' : text);
  let pre = text.slice(0, idx);
  const mid = text.slice(idx, idx + query.length);
  let post = text.slice(idx + query.length);
  if (pre.length > 16) pre = '…' + pre.slice(-16);
  if (post.length > 38) post = post.slice(0, 38) + '…';
  return escapeHtml(pre) + '<span class="wmark">' + escapeHtml(mid) + '</span>' + escapeHtml(post);
}

interface MainPaneProps {
  lang: BufferMeta['lang'];
  content: string;
  viewMode: 'source' | 'split' | 'preview';
  narrow: boolean;
  zoom: number;
  gutterRef: React.RefObject<HTMLPreElement>;
  previewRef: React.RefObject<HTMLDivElement>;
  onEdit: (e: React.ChangeEvent<HTMLTextAreaElement>) => void;
  onCaret: (e: React.SyntheticEvent<HTMLTextAreaElement>) => void;
  onGutterSync: (e: React.UIEvent<HTMLTextAreaElement>) => void;
}

function MainPane(props: MainPaneProps): ReactNode {
  const { lang, content, viewMode, narrow, zoom, gutterRef, previewRef } = props;
  const zoomStyle: CSSProperties = {
    zoom,
    height: '100%',
    minHeight: 0,
    display: 'flex',
    flexDirection: 'column',
    flex: 1,
  };

  if (lang === 'binary') {
    const rows = genHex();
    return (
      <div style={zoomStyle}>
        <div className="ww-hex codescroll">
          <div className="ww-hex-inner">
            {rows.map((r, k) => (
              <div className="ww-hex-row" key={k}>
                <span style={{ color: 'var(--subtle)' }}>{r.off}</span>
                <span style={{ color: 'var(--foreground)' }}>{r.left + '  ' + r.right}</span>
                <span style={{ color: 'var(--muted)' }}>{'|' + r.ascii + '|'}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    );
  }

  if (lang === 'huge') {
    const lines = content.split('\n');
    const nums = lines.map((_, k) => k + 1).join('\n');
    return (
      <div style={zoomStyle}>
        <div className="ww-huge codescroll">
          <div className="ww-codeflex">
            <pre className="ww-gutter ww-gutter-sticky">{nums}</pre>
            <pre className="ww-huge-body">{content}</pre>
          </div>
        </div>
      </div>
    );
  }

  const hasPreview = lang === 'md' || lang === 'html';
  const vm = hasPreview ? viewMode : 'source';
  const showEditor = !hasPreview || vm === 'source' || vm === 'split';
  const showPreview = hasPreview && (vm === 'preview' || vm === 'split');
  const showEditorHere = showEditor && !(narrow && vm === 'split');
  const editable = lang === 'md';
  const lines = content.split('\n');
  const nums = lines.map((_, k) => k + 1).join('\n');

  let editorPane: ReactNode = null;
  if (editable) {
    editorPane = (
      <div className="ww-editor">
        <pre ref={gutterRef} aria-hidden="true" className="ww-gutter ww-gutter-edit">
          {nums}
        </pre>
        <textarea
          value={content}
          onChange={props.onEdit}
          onScroll={props.onGutterSync}
          onKeyUp={props.onCaret}
          onClick={props.onCaret}
          spellCheck={false}
          aria-label="Markdown source, editable"
          className="ww-textarea"
        />
      </div>
    );
  } else {
    const codeHtml = highlightCode(lang === 'ts' ? 'ts' : lang === 'html' ? 'html' : 'plain', content);
    editorPane = (
      <div className="ww-codepane codescroll">
        <div className="ww-codeflex">
          <pre className="ww-gutter ww-gutter-sticky">{nums}</pre>
          <pre className="ww-code-body" dangerouslySetInnerHTML={{ __html: codeHtml }} />
        </div>
      </div>
    );
  }

  let previewPane: ReactNode = null;
  if (showPreview) {
    // XSS boundary: mdToHtml escapes all input; the raw-HTML branch only runs for the
    // static release-email.html demo buffer, which is never editable (editable === md).
    const html = lang === 'html' ? content : mdToHtml(content);
    previewPane = (
      <div
        ref={previewRef}
        className="writ-prose ww-preview"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    );
  }

  if (showPreview && showEditorHere) {
    return (
      <div style={zoomStyle}>
        <div className="ww-split">
          <div className="ww-split-edit">{editorPane}</div>
          {previewPane}
        </div>
      </div>
    );
  }
  if (showPreview) return <div style={zoomStyle}>{previewPane}</div>;
  return <div style={zoomStyle}>{editorPane}</div>;
}
