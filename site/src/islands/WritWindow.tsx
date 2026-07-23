import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from 'react';
import { EditorView } from '@codemirror/view';
import {
  BUFFERS,
  DEFAULT_CONTENTS,
  FMT,
  HISTORY,
  OPEN_FILES,
  TEXT_TRANSFORMS,
  cmLangId,
  type BufferLang,
  type BufferMeta,
} from './writ/buffers';
import { escapeHtml, estimateTokens, mdToHtml } from './writ/render';
import { genHex } from './writ/hex';
import { applyTransform } from './writ/transforms';
import { checkSpelling } from './writ/spellcheck';
import {
  applySpellingFixes,
  computeFixChanges,
  createDemoState,
  reconfigureSpelling,
  reconfigureTheme,
  setSpellingLints,
  spellingEntries,
  type Polarity,
} from './writ/cm';
import { EDITOR_COMMANDS } from '@app/editor/editor-command-table';
import {
  insertLink,
  toggleBold,
  toggleInlineCode,
  toggleItalic,
  toggleStrikethrough,
} from '@app/commands/markdown-format';
import { keybindingSegments } from '@app/lib/keybinding-format';
import { languageLabel } from '@app/components/Editor/language-label';
import '../styles/writ-window.css';

// Matches a task-list line, capturing (prefix)(state char)(rest). Kept in step
// with render.ts so click index and source order agree.
const TASK_LINE = /^(\s*[-*+]\s+\[)([ xX])(\]\s.*)$/;

const HOTKEY_TOGGLE = 'CmdOrCtrl+Shift+Space';

type ViewMode = 'source' | 'split' | 'preview';
type SaveStatus = 'idle' | 'saved';

interface Caret {
  ln: number;
  col: number;
}

interface SearchHit {
  id: string;
  line: number;
  text: string;
}

interface DemoCommand {
  id: string;
  name: string;
  description?: string;
  binding?: string;
  scope: 'app' | 'editor';
  run: () => void;
}

// Matches src/stores/global/token-estimate.ts formatTokenCount.
function formatTokens(count: number): string {
  if (count < 1000) return String(count);
  const thousands = count / 1000;
  if (thousands < 10) {
    const rounded = Math.round(thousands * 10) / 10;
    if (rounded >= 10) return '10k';
    return Number.isInteger(rounded) ? `${rounded}k` : `${rounded.toFixed(1)}k`;
  }
  return `${Math.round(thousands)}k`;
}

function currentPolarity(): Polarity {
  return document.documentElement.getAttribute('data-theme') === 'dark' ? 'dark' : 'light';
}

function mermaidThemeVars(isDark: boolean) {
  return {
    primaryColor: isDark ? '#1a1a2e' : '#eef1f7',
    primaryBorderColor: isDark ? '#8aa6ff' : '#3b5bdb',
    primaryTextColor: isDark ? '#e0e0e0' : '#1a1a22',
    lineColor: '#8a8aa0',
    fontSize: '12px',
  };
}

function Keys({ binding, showEmpty = false }: { binding?: string; showEmpty?: boolean }): ReactNode {
  const segments = binding ? keybindingSegments(binding) : [];
  if (segments.length === 0) {
    // Mirrors the app's Kbd: a muted em dash where a command has no shortcut.
    return showEmpty ? (
      <span className="wwx-keys wwx-keys-muted" aria-hidden="true">
        <span className="wwx-key wwx-key-empty">—</span>
      </span>
    ) : null;
  }
  return (
    <span className="wwx-keys" aria-hidden="true">
      {segments.map((seg, i) => (
        <span key={i} className="wwx-key">
          {seg}
        </span>
      ))}
    </span>
  );
}

export default function WritWindow() {
  const contentsRef = useRef<Map<string, string>>(new Map(Object.entries(DEFAULT_CONTENTS)));
  const dynamicRef = useRef<Record<string, BufferMeta>>({});
  const newCountRef = useRef(0);
  const viewRef = useRef<EditorView | null>(null);
  const cmBufferRef = useRef<string | null>(null);
  const loadingRef = useRef(false);
  const spellingOnRef = useRef(false);
  const hoverRef = useRef(false);
  const lastShiftRef = useRef(0);
  const lastMermaidRef = useRef('');
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const relintTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const rootRef = useRef<HTMLDivElement>(null);
  const editorHostRef = useRef<HTMLDivElement>(null);
  const previewRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const paletteRef = useRef<HTMLInputElement>(null);
  const renameRef = useRef<HTMLInputElement>(null);

  const [activeId, setActiveId] = useState('report.md');
  const [tabs, setTabs] = useState<string[]>(() => OPEN_FILES.slice());
  const [names, setNames] = useState<Record<string, string>>({});
  const [viewMode, setViewMode] = useState<ViewMode>('split');
  const [doc, setDoc] = useState<string>(() => DEFAULT_CONTENTS['report.md'] ?? '');
  const [caret, setCaret] = useState<Caret>({ ln: 1, col: 1 });
  const [saveStatus, setSaveStatus] = useState<SaveStatus>('idle');
  const [query, setQuery] = useState('');
  const [searchMs, setSearchMs] = useState(0);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [paletteQuery, setPaletteQuery] = useState('');
  const [paletteIndex, setPaletteIndex] = useState(0);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [zoom, setZoom] = useState(1);
  const [watching, setWatching] = useState(false);
  const [narrow, setNarrow] = useState(false);
  const [spellingOn, setSpellingOn] = useState(false);
  const [spellCount, setSpellCount] = useState(0);
  const [spellMenuOpen, setSpellMenuOpen] = useState(false);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameTemp, setRenameTemp] = useState('');
  const [recent, setRecent] = useState<string[]>(['togglesidebar', 'search', 'copyprompt', 'newtab']);

  const metaOf = useCallback(
    (id: string): BufferMeta =>
      dynamicRef.current[id] ?? BUFFERS[id] ?? (BUFFERS['report.md'] as BufferMeta),
    [],
  );
  const nameOf = useCallback(
    (id: string): string => names[id] || metaOf(id).name || id,
    [names, metaOf],
  );
  const defaultView = useCallback(
    (id: string): ViewMode => {
      const lang = metaOf(id).lang;
      return lang === 'md' || lang === 'html' ? 'split' : 'source';
    },
    [metaOf],
  );

  const spellingEligible = useCallback(
    (lang: BufferLang): boolean => lang !== 'binary' && lang !== 'huge',
    [],
  );

  const bufText = useCallback((id: string): string => {
    if (id === cmBufferRef.current && viewRef.current) {
      return viewRef.current.state.doc.toString();
    }
    return contentsRef.current.get(id) ?? '';
  }, []);

  // Mirrors the app: "saved" appears after the autosave settles, not while the
  // user is mid-keystroke. Each edit hides it and restarts the quiet window;
  // once quiet, it shows, then fades back to idle.
  const markSaved = useCallback(() => {
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    setSaveStatus('idle');
    saveTimerRef.current = setTimeout(() => {
      setSaveStatus('saved');
      saveTimerRef.current = setTimeout(() => setSaveStatus('idle'), 1400);
    }, 800);
  }, []);

  const scheduleRelint = useCallback((view: EditorView) => {
    if (relintTimerRef.current) clearTimeout(relintTimerRef.current);
    relintTimerRef.current = setTimeout(() => {
      relintTimerRef.current = null;
      if (!spellingOnRef.current) return;
      view.dispatch({ effects: setSpellingLints.of(checkSpelling(view.state.doc.toString())) });
    }, 150);
  }, []);

  const onUpdate = useCallback(
    (update: import('@codemirror/view').ViewUpdate) => {
      const sel = update.state.selection.main;
      const line = update.state.doc.lineAt(sel.head);
      setCaret({ ln: line.number, col: sel.head - line.from + 1 });
      if (update.docChanged) {
        setDoc(update.state.doc.toString());
        if (!loadingRef.current) {
          markSaved();
          if (spellingOnRef.current) scheduleRelint(update.view);
        }
      }
    },
    [markSaved, scheduleRelint],
  );

  const loadBuffer = useCallback(
    (id: string) => {
      const view = viewRef.current;
      if (!view) return;
      const prev = cmBufferRef.current;
      if (prev && metaOf(prev).lang !== 'binary') {
        contentsRef.current.set(prev, view.state.doc.toString());
      }
      const b = metaOf(id);
      if (b.lang === 'binary') {
        setDoc('');
        setCaret({ ln: 1, col: 1 });
        setSpellCount(0);
        return;
      }
      const content = contentsRef.current.get(id) ?? '';
      const langId = cmLangId(b.lang);
      const restricted = b.lang === 'huge';
      const eligible = spellingEligible(b.lang);
      const spellOn = spellingOnRef.current && eligible;
      loadingRef.current = true;
      view.setState(
        createDemoState({
          content,
          langId,
          restricted,
          polarity: currentPolarity(),
          spelling: spellOn,
          onSpellCount: setSpellCount,
          onUpdate,
          onToggleSidebar: () => setSidebarOpen((s) => !s),
          onFocusSearch: () => {
            setSidebarOpen(true);
            setTimeout(() => searchRef.current?.focus(), 0);
          },
        }),
      );
      loadingRef.current = false;
      cmBufferRef.current = id;
      setDoc(content);
      setCaret({ ln: 1, col: 1 });
      setSpellCount(0);
      if (spellOn) {
        view.dispatch({ effects: setSpellingLints.of(checkSpelling(content)) });
      }
      requestAnimationFrame(() => viewRef.current?.requestMeasure());
      view.focus();
    },
    [metaOf, onUpdate, spellingEligible],
  );

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
      setSpellMenuOpen(false);
      loadBuffer(id);
    },
    [defaultView, loadBuffer],
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
          loadBuffer(fallback);
          return fallback;
        });
        return next;
      });
    },
    [defaultView, loadBuffer],
  );

  const cmdNewTab = useCallback(() => {
    newCountRef.current += 1;
    const id = 'scratch-' + newCountRef.current;
    dynamicRef.current[id] = { name: 'untitled-' + newCountRef.current + '.md', lang: 'md' };
    contentsRef.current.set(id, '');
    setPaletteOpen(false);
    setTabs((prev) => [...prev, id]);
    setActiveId(id);
    setViewMode('split');
    loadBuffer(id);
  }, [loadBuffer]);

  const runCm = useCallback((command: (view: EditorView) => boolean) => {
    const view = viewRef.current;
    if (!view) return;
    view.focus();
    command(view);
  }, []);

  const focusSearch = useCallback(() => {
    setSidebarOpen(true);
    setPaletteOpen(false);
    setTimeout(() => searchRef.current?.focus(), 50);
  }, []);

  const computeResults = useCallback(
    (q: string): SearchHit[] => {
      const trimmed = q.trim();
      if (!trimmed) return [];
      const lc = trimmed.toLowerCase();
      const res: SearchHit[] = [];
      for (const [id, b] of Object.entries(BUFFERS)) {
        if (b.lang === 'binary') continue;
        const lines = bufText(id).split('\n');
        for (let i = 0; i < lines.length; i++) {
          if ((lines[i] ?? '').toLowerCase().indexOf(lc) >= 0) {
            res.push({ id, line: i + 1, text: lines[i] ?? '' });
            break;
          }
        }
      }
      return res;
    },
    [bufText],
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

  const toggleTask = useCallback((index: number) => {
    const view = viewRef.current;
    if (!view) return;
    const lines = view.state.doc.toString().split('\n');
    let n = 0;
    for (let li = 0; li < lines.length; li++) {
      const m = (lines[li] ?? '').match(TASK_LINE);
      if (!m) continue;
      if (n === index) {
        const lineStart = view.state.doc.line(li + 1).from;
        const boxPos = lineStart + (m[1] ?? '').length;
        const nextChar = (m[2] ?? '').toLowerCase() === 'x' ? ' ' : 'x';
        view.dispatch({ changes: { from: boxPos, to: boxPos + 1, insert: nextChar }, userEvent: 'input' });
        return;
      }
      n += 1;
    }
  }, []);

  const onPreviewClick = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      const hit = (e.target as HTMLElement).closest('[data-task]');
      if (!hit) return;
      const index = Number(hit.getAttribute('data-task'));
      if (Number.isNaN(index)) return;
      toggleTask(index);
    },
    [toggleTask],
  );

  const copyPrompt = useCallback(() => {
    const out = applyTransform('prompt', bufText(activeId));
    try {
      void navigator.clipboard?.writeText(out);
    } catch {
      /* clipboard unavailable */
    }
    setPaletteOpen(false);
  }, [activeId, bufText]);

  const startRename = useCallback(
    (id: string) => {
      setPaletteOpen(false);
      setRenamingId(id);
      setRenameTemp(nameOf(id));
      setTimeout(() => {
        renameRef.current?.focus();
        renameRef.current?.select();
      }, 30);
    },
    [nameOf],
  );

  const commitRename = useCallback(() => {
    setRenamingId((id) => {
      if (!id) return null;
      const nm = renameTemp.trim() || nameOf(id);
      setNames((prev) => ({ ...prev, [id]: nm }));
      return null;
    });
  }, [renameTemp, nameOf]);

  const setSpelling = useCallback(
    (next: boolean) => {
      const view = viewRef.current;
      spellingOnRef.current = next;
      setSpellingOn(next);
      setSpellMenuOpen(false);
      if (!view) return;
      reconfigureSpelling(view, next, setSpellCount);
      if (next) {
        view.dispatch({ effects: setSpellingLints.of(checkSpelling(view.state.doc.toString())) });
      } else {
        setSpellCount(0);
      }
    },
    [],
  );

  const fixAllSpelling = useCallback(() => {
    const view = viewRef.current;
    if (!view) return;
    setSpellMenuOpen(false);
    const entries = spellingEntries(view.state);
    const fixes = computeFixChanges(entries, (from, to) => view.state.doc.sliceString(from, to));
    applySpellingFixes(view, fixes);
    scheduleRelint(view);
  }, [scheduleRelint]);

  const zoomIn = useCallback(() => {
    setZoom((z) => Math.min(1.6, Math.round((z + 0.1) * 10) / 10));
    setPaletteOpen(false);
  }, []);
  const zoomOut = useCallback(() => {
    setZoom((z) => Math.max(0.7, Math.round((z - 0.1) * 10) / 10));
    setPaletteOpen(false);
  }, []);
  const zoomReset = useCallback(() => {
    setZoom(1);
    setPaletteOpen(false);
  }, []);

  const b = metaOf(activeId);
  const lang = b.lang;
  const hasPreview = lang === 'md' || lang === 'html';
  const editableMd = lang === 'md';
  const eligible = spellingEligible(lang);

  const commands = useMemo<DemoCommand[]>(() => {
    const list: DemoCommand[] = [
      { id: 'newtab', name: 'New Tab', description: 'Open a fresh scratch buffer.', binding: 'CmdOrCtrl+T', scope: 'app', run: cmdNewTab },
      { id: 'closetab', name: 'Close Tab', description: 'Close the active buffer.', binding: 'CmdOrCtrl+W', scope: 'app', run: () => closeTab(activeId) },
      { id: 'nexttab', name: 'Next Tab', description: 'Jump to the next open buffer.', binding: 'CmdOrCtrl+]', scope: 'app', run: () => {
        const i = tabs.indexOf(activeId);
        open(tabs[(i + 1) % tabs.length] ?? activeId);
      } },
      { id: 'prevtab', name: 'Previous Tab', description: 'Jump to the previous open buffer.', binding: 'CmdOrCtrl+[', scope: 'app', run: () => {
        const i = tabs.indexOf(activeId);
        open(tabs[(i - 1 + tabs.length) % tabs.length] ?? activeId);
      } },
      { id: 'renametab', name: 'Rename Tab', description: 'Rename the active buffer inline.', binding: 'F2', scope: 'app', run: () => startRename(activeId) },
      { id: 'togglesidebar', name: 'Toggle Sidebar', description: 'Show or hide the buffer list.', binding: 'CmdOrCtrl+S', scope: 'app', run: () => { setSidebarOpen((s) => !s); setPaletteOpen(false); } },
      { id: 'find', name: 'Find', description: 'Focus the search field.', binding: 'CmdOrCtrl+F', scope: 'app', run: focusSearch },
      { id: 'search', name: 'Search', description: 'Full-text search across every buffer.', scope: 'app', run: focusSearch },
      { id: 'zoomin', name: 'Zoom In', description: 'Increase editor scale.', binding: 'CmdOrCtrl+=', scope: 'app', run: zoomIn },
      { id: 'zoomout', name: 'Zoom Out', description: 'Decrease editor scale.', binding: 'CmdOrCtrl+-', scope: 'app', run: zoomOut },
      { id: 'zoomreset', name: 'Reset Zoom', description: 'Return to 100%.', binding: 'CmdOrCtrl+0', scope: 'app', run: zoomReset },
      { id: 'watchinbox', name: 'Watch Inbox', description: 'Auto-open files dropped into the inbox folder.', scope: 'app', run: () => { setWatching((w) => !w); setSidebarOpen(true); setPaletteOpen(false); } },
      { id: 'copyprompt', name: 'Copy as Prompt', description: 'Copy the buffer, cleaned, to the clipboard.', scope: 'app', run: copyPrompt },
    ];
    for (const t of TEXT_TRANSFORMS) {
      list.push({
        id: t.id,
        name: `Text: ${t.name}`,
        description: t.desc,
        scope: 'app',
        run: () => {
          const view = viewRef.current;
          if (!view) return;
          const next = applyTransform(t.id, view.state.doc.toString());
          view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: next }, userEvent: 'input' });
          setPaletteOpen(false);
        },
      });
    }
    if (editableMd) {
      const fmt: [string, string, string, (v: EditorView) => boolean][] = [
        ['editor.toggleBold', 'Toggle Bold', 'CmdOrCtrl+B', toggleBold],
        ['editor.toggleItalic', 'Toggle Italic', 'CmdOrCtrl+I', toggleItalic],
        ['editor.toggleStrikethrough', 'Toggle Strikethrough', 'CmdOrCtrl+Shift+X', toggleStrikethrough],
        ['editor.toggleInlineCode', 'Toggle Inline Code', 'CmdOrCtrl+Shift+E', toggleInlineCode],
        ['editor.insertLink', 'Insert Link', 'CmdOrCtrl+K', insertLink],
      ];
      for (const [id, name, binding, cmd] of fmt) {
        list.push({ id, name, binding, scope: 'editor', run: () => { runCm(cmd); setPaletteOpen(false); } });
      }
    }
    for (const spec of EDITOR_COMMANDS) {
      list.push({
        id: spec.id,
        name: spec.label,
        binding: spec.keybinding,
        scope: 'editor',
        run: () => { runCm(spec.run); setPaletteOpen(false); },
      });
    }
    return list;
  }, [activeId, tabs, editableMd, cmdNewTab, closeTab, open, startRename, focusSearch, zoomIn, zoomOut, zoomReset, copyPrompt, runCm]);

  // Empty query: Recent (usage-ranked), then Commands (app scope), then Editor
  // (editor scope) — matching the app's palette grouping. With a query: a single
  // unlabeled ranked-results section.
  const paletteSections = useMemo(() => {
    const pq = paletteQuery.trim().toLowerCase();
    if (pq) {
      const matches = commands.filter(
        (c) => (c.name + ' ' + (c.description ?? '')).toLowerCase().includes(pq),
      );
      return [{ label: null as string | null, commands: matches }];
    }
    const byId = new Map(commands.map((c) => [c.id, c]));
    const recentCmds = recent.map((id) => byId.get(id)).filter((c): c is DemoCommand => Boolean(c));
    const recentIds = new Set(recentCmds.map((c) => c.id));
    const rest = commands.filter((c) => !recentIds.has(c.id));
    const sections: { label: string | null; commands: DemoCommand[] }[] = [];
    if (recentCmds.length > 0) sections.push({ label: 'Recent', commands: recentCmds });
    const appRest = rest.filter((c) => c.scope === 'app');
    const editorRest = rest.filter((c) => c.scope === 'editor');
    if (appRest.length > 0) sections.push({ label: 'Commands', commands: appRest });
    if (editorRest.length > 0) sections.push({ label: 'Editor', commands: editorRest });
    return sections;
  }, [commands, paletteQuery, recent]);

  const paletteFlat = useMemo(
    () => paletteSections.flatMap((s) => s.commands),
    [paletteSections],
  );

  useEffect(() => {
    setPaletteIndex(0);
  }, [paletteQuery, paletteOpen]);

  const runCommand = useCallback((cmd: DemoCommand) => {
    cmd.run();
    setRecent((prev) => [cmd.id, ...prev.filter((x) => x !== cmd.id)].slice(0, 5));
  }, []);

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

  const loadFmt = useCallback(
    (k: 'md' | 'html' | 'mermaid' | 'math') => {
      open(FMT[k]);
      scrollWin();
    },
    [open, scrollWin],
  );

  // Create the persistent EditorView once. StrictMode's mount/unmount/mount
  // cycle destroys and recreates cleanly; the guard blocks a stray re-create
  // that would orphan a live view.
  useEffect(() => {
    if (viewRef.current) return;
    const host = editorHostRef.current;
    if (!host) return;
    const view = new EditorView({
      state: createDemoState({
        content: DEFAULT_CONTENTS['report.md'] ?? '',
        langId: 'markdown',
        restricted: false,
        polarity: currentPolarity(),
        spelling: false,
        onSpellCount: setSpellCount,
        onUpdate,
        onToggleSidebar: () => setSidebarOpen((s) => !s),
        onFocusSearch: () => {
          setSidebarOpen(true);
          setTimeout(() => searchRef.current?.focus(), 0);
        },
      }),
      parent: host,
    });
    viewRef.current = view;
    cmBufferRef.current = 'report.md';
    return () => {
      view.destroy();
      viewRef.current = null;
      cmBufferRef.current = null;
    };
  }, [onUpdate]);

  useEffect(() => {
    const onTheme = () => {
      const view = viewRef.current;
      if (view) reconfigureTheme(view, currentPolarity());
    };
    document.addEventListener('writ:theme', onTheme);
    return () => document.removeEventListener('writ:theme', onTheme);
  }, []);

  // Reapply CM measurement after a view-mode change re-shows a hidden editor.
  useEffect(() => {
    requestAnimationFrame(() => viewRef.current?.requestMeasure());
  }, [viewMode, sidebarOpen]);

  // Latest handlers, read by the once-registered document listeners below so
  // they never re-subscribe or capture stale closures.
  const handlers = { open, openPalette, togglePalette, runSearch, loadFmt, scrollWin, paletteOpen };
  const handlersRef = useRef(handlers);
  handlersRef.current = handlers;

  useEffect(() => {
    const onResize = () => setNarrow(window.innerWidth < 720);
    onResize();
    window.addEventListener('resize', onResize);

    const onKey = (e: KeyboardEvent) => {
      const h = handlersRef.current;
      if (e.key === 'Shift' && !e.repeat && !e.metaKey && !e.ctrlKey && !e.altKey) {
        const now = performance.now();
        const within = now - lastShiftRef.current < 400;
        lastShiftRef.current = now;
        if (within) {
          const focusWithin = rootRef.current?.contains(document.activeElement);
          if (hoverRef.current || focusWithin || h.paletteOpen) {
            e.preventDefault();
            lastShiftRef.current = 0;
            h.togglePalette();
          }
        }
        return;
      }
      lastShiftRef.current = 0;
      if (e.key === 'Escape' && h.paletteOpen) setPaletteOpen(false);
    };
    document.addEventListener('keydown', onKey);

    const onCmd = (e: Event) => {
      const detail = (e as CustomEvent).detail as { action: string; arg?: string };
      if (!detail) return;
      const h = handlersRef.current;
      if (detail.action === 'loadFmt') h.loadFmt(detail.arg as 'md' | 'html' | 'mermaid' | 'math');
      else if (detail.action === 'open' && detail.arg) {
        h.open(detail.arg);
        h.scrollWin();
      } else if (detail.action === 'palette') h.openPalette();
      else if (detail.action === 'demoRender') {
        h.open('report.md');
        setViewMode('split');
        setQuery('');
        h.scrollWin();
      } else if (detail.action === 'search') {
        setSidebarOpen(true);
        h.runSearch(detail.arg || 'settle');
        h.scrollWin();
        setTimeout(() => searchRef.current?.focus(), 60);
      }
    };
    document.addEventListener('writ:cmd', onCmd);

    return () => {
      window.removeEventListener('resize', onResize);
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('writ:cmd', onCmd);
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      if (relintTimerRef.current) clearTimeout(relintTimerRef.current);
    };
  }, []);

  // KaTeX + Mermaid render for the markdown preview (lazy imports, unchanged).
  useEffect(() => {
    let cancelled = false;
    const el = previewRef.current;
    if (!el) return;
    const isDark = currentPolarity() === 'dark';

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
        if (sig === lastMermaidRef.current) return;
        lastMermaidRef.current = sig;
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
      lastMermaidRef.current = '';
      void renderLibs();
    };
    document.addEventListener('writ:theme', onTheme);
    return () => {
      cancelled = true;
      document.removeEventListener('writ:theme', onTheme);
    };
  }, [doc, activeId, viewMode]);

  const results = computeResults(query);
  const showResults = query.trim().length > 0;
  const tokenText = lang === 'binary' ? null : formatTokens(estimateTokens(doc));
  const largeFileLabel =
    lang === 'binary' ? 'Binary · read-only' : lang === 'huge' ? 'Large file · syntax off' : null;
  const langLabelText = languageLabel(cmLangId(lang));

  const spellLabel = !spellingOn ? 'Spelling off' : spellCount > 0 ? `${spellCount} spelling` : 'Spelling';

  const bodyStyle: CSSProperties = { gridTemplateColumns: sidebarOpen ? '232px minmax(0,1fr)' : 'minmax(0,1fr)' };
  const stageStyle: CSSProperties = zoom !== 1 ? ({ zoom } as CSSProperties) : {};

  return (
    <div ref={rootRef} className="ww-root">
      <div
        className="ww-window"
        onMouseEnter={() => (hoverRef.current = true)}
        onMouseLeave={() => (hoverRef.current = false)}
      >
        <div className="wwx-titlebar">
          <div className="wwx-lights" aria-hidden="true">
            <span style={{ background: 'var(--writ-traffic-close)' }} />
            <span style={{ background: 'var(--writ-traffic-minimize)' }} />
            <span style={{ background: 'var(--writ-traffic-maximize)' }} />
          </div>
          <div className="wwx-tabs sidescroll">
            {tabs.map((tid) => (
              <div
                key={tid}
                className="wwx-tab"
                role="button"
                tabIndex={0}
                aria-pressed={tid === activeId}
                data-active={tid === activeId}
                onClick={() => open(tid)}
                onDoubleClick={(e) => {
                  e.stopPropagation();
                  startRename(tid);
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    open(tid);
                  }
                }}
                title={nameOf(tid)}
              >
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
                    className="wwx-tab-rename"
                  />
                ) : (
                  <span className="wwx-tab-title">{nameOf(tid)}</span>
                )}
                <span
                  className="wwx-tab-close"
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
            <button className="wwx-newtab" onClick={cmdNewTab} title="New tab" aria-label="New tab" type="button">
              +
            </button>
          </div>
          <div className="wwx-chord" title="Toggle Writ from anywhere">
            <Keys binding={HOTKEY_TOGGLE} />
          </div>
        </div>

        <div className="wwx-body" style={bodyStyle}>
          {sidebarOpen && (
            <aside className="wwx-side">
              <div className="wwx-search">
                <svg className="wwx-search-icon" width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
                  <circle cx="6" cy="6" r="4" stroke="currentColor" strokeWidth="1.4" fill="none" />
                  <path d="M9 9L12.5 12.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
                </svg>
                <input
                  ref={searchRef}
                  value={query}
                  onChange={(e) => runSearch(e.target.value)}
                  placeholder="Search buffers..."
                  aria-label="Search buffers"
                  autoComplete="off"
                  spellCheck={false}
                  className="wwx-search-input"
                />
                {showResults && (
                  <span className="wwx-search-count" aria-live="polite">
                    {results.length === 1 ? '1 result' : `${results.length} results`}
                  </span>
                )}
              </div>
              <div className="wwx-side-scroll sidescroll">
                {showResults ? (
                  <div className="wwx-section">
                    <div className="wwx-section-title">Results</div>
                    {results.map((r) => (
                      <button
                        key={r.id}
                        className="wwx-srow"
                        data-active={r.id === activeId}
                        onClick={() => open(r.id)}
                      >
                        <span className="wwx-srow-name">{nameOf(r.id)}</span>
                        <span
                          className="wwx-srow-line"
                          dangerouslySetInnerHTML={{ __html: highlightHit(r.text.trim(), query) }}
                        />
                        <span className="wwx-srow-loc">L{r.line}</span>
                      </button>
                    ))}
                    <div className="wwx-foot">
                      {results.length} of {results.length} · {searchMs} ms
                    </div>
                  </div>
                ) : (
                  <>
                    <div className="wwx-section">
                      <div className="wwx-section-head">
                        <span className="wwx-section-title">Active</span>
                        {watching && (
                          <span className="wwx-watch">
                            <span className="wwx-watch-dot" />
                            watching inbox
                          </span>
                        )}
                      </div>
                      {OPEN_FILES.map((fid) => (
                        <button
                          key={fid}
                          className="wwx-row"
                          data-active={fid === activeId}
                          onClick={() => open(fid)}
                        >
                          {nameOf(fid)}
                        </button>
                      ))}
                    </div>
                    <div className="wwx-section">
                      <div className="wwx-section-title">History</div>
                      {HISTORY.map((h) => (
                        <button
                          key={h.id}
                          className="wwx-row wwx-row-hist"
                          data-active={h.id === activeId}
                          onClick={() => open(h.id)}
                        >
                          <span className="wwx-row-name">{nameOf(h.id)}</span>
                          <span className="wwx-row-when">{h.when}</span>
                        </button>
                      ))}
                    </div>
                  </>
                )}
              </div>
            </aside>
          )}

          <div
            className="wwx-main"
            data-view={hasPreview ? viewMode : 'source'}
            data-binary={lang === 'binary'}
            data-narrow={narrow}
          >
            <div className="wwx-stage" style={stageStyle}>
              <div ref={editorHostRef} className="wwx-editor-host" />
              {lang === 'binary' && <HexPane />}
              {hasPreview && lang === 'md' && (
                <div
                  ref={previewRef}
                  className="writ-prose wwx-preview"
                  onClick={onPreviewClick}
                  dangerouslySetInnerHTML={{ __html: mdToHtml(doc) }}
                />
              )}
              {hasPreview && lang === 'html' && (
                <iframe className="wwx-preview wwx-html-frame" sandbox="" srcDoc={doc} title="HTML preview" />
              )}
            </div>
          </div>

          {paletteOpen && (
            <div className="wwx-scrim" onClick={() => setPaletteOpen(false)}>
              <div
                className="wwx-palette"
                onClick={(e) => e.stopPropagation()}
                role="dialog"
                aria-modal="true"
                aria-label="Command palette"
              >
                <input
                  ref={paletteRef}
                  value={paletteQuery}
                  onChange={(e) => setPaletteQuery(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'ArrowDown') {
                      e.preventDefault();
                      setPaletteIndex((i) => Math.min(i + 1, paletteFlat.length - 1));
                    } else if (e.key === 'ArrowUp') {
                      e.preventDefault();
                      setPaletteIndex((i) => Math.max(i - 1, 0));
                    } else if (e.key === 'Enter') {
                      e.preventDefault();
                      const cmd = paletteFlat[paletteIndex];
                      if (cmd) runCommand(cmd);
                    }
                  }}
                  placeholder="Search commands"
                  aria-label="Command search"
                  autoComplete="off"
                  spellCheck={false}
                  className="wwx-palette-input"
                />
                <div className="wwx-palette-list sidescroll">
                  {paletteFlat.length === 0 && (
                    <div className="wwx-palette-empty">Nothing matches "{paletteQuery}"</div>
                  )}
                  {paletteSections.map((section) => (
                    <div key={section.label ?? 'results'} className="wwx-palette-section">
                      {section.label && <div className="wwx-palette-label">{section.label}</div>}
                      {section.commands.map((cmd) => {
                        const idx = paletteFlat.indexOf(cmd);
                        return (
                          <button
                            key={cmd.id}
                            className="wwx-pcmd"
                            data-selected={idx === paletteIndex}
                            onClick={() => runCommand(cmd)}
                            onMouseMove={() => setPaletteIndex(idx)}
                          >
                            <span className="wwx-pcmd-text">
                              <span className="wwx-pcmd-name">{cmd.name}</span>
                              {cmd.description && <span className="wwx-pcmd-desc">{cmd.description}</span>}
                            </span>
                            <Keys binding={cmd.binding} showEmpty />

                          </button>
                        );
                      })}
                    </div>
                  ))}
                </div>
              </div>
            </div>
          )}
        </div>

        <div className="wwx-status">
          <div className="wwx-status-left" role="status" aria-live="polite">
            {saveStatus === 'saved' && (
              <span className="wwx-save">
                <span className="wwx-save-dot" aria-hidden="true" />
                saved
              </span>
            )}
            {largeFileLabel && <span className="wwx-chip">{largeFileLabel}</span>}
          </div>
          <div className="wwx-status-right">
            <span className="wwx-field">
              Ln {caret.ln}, Col {caret.col}
            </span>
            <span className="wwx-field hide-sm">{langLabelText}</span>
            <span className="wwx-field hide-sm">UTF-8</span>
            {eligible && (
              <div className="wwx-spell-wrap">
                <button
                  type="button"
                  className="wwx-chip wwx-spell"
                  data-off={!spellingOn}
                  onClick={() => setSpellMenuOpen((o) => !o)}
                  aria-label={spellLabel}
                >
                  {spellLabel}
                </button>
                {spellMenuOpen && (
                  <div className="wwx-menu" role="menu">
                    {!spellingOn ? (
                      <button type="button" className="wwx-menu-item" onClick={() => setSpelling(true)}>
                        Turn on spelling
                      </button>
                    ) : (
                      <>
                        <button type="button" className="wwx-menu-item" onClick={() => setSpelling(false)}>
                          Turn off spelling
                        </button>
                        {spellCount > 0 && (
                          <button type="button" className="wwx-menu-item" onClick={fixAllSpelling}>
                            Fix all ({spellCount})
                          </button>
                        )}
                      </>
                    )}
                  </div>
                )}
              </div>
            )}
            {tokenText && <span className="wwx-tokens">≈ {tokenText} tok</span>}
            {zoom !== 1 && <span className="wwx-field">{Math.round(zoom * 100)}%</span>}
            {hasPreview && (
              <div className="wwx-viewtoggle" role="group" aria-label="Preview layout">
                <button className="wwx-vtog" aria-pressed={viewMode === 'source'} title="Source" onClick={() => setViewMode('source')}>
                  Source
                </button>
                <button className="wwx-vtog" aria-pressed={viewMode === 'split'} title="Split" onClick={() => setViewMode('split')}>
                  Split
                </button>
                <button className="wwx-vtog" aria-pressed={viewMode === 'preview'} title="Preview" onClick={() => setViewMode('preview')}>
                  Preview
                </button>
              </div>
            )}
            <button className="wwx-palette-cue" onClick={openPalette} aria-label="Open command palette" type="button">
              <Keys binding="Shift+Shift" />
              <span className="wwx-palette-cue-label">command palette</span>
            </button>
          </div>
        </div>
      </div>
      <p className="ww-caption">
        The real editor. Type in <span className="ww-mono">report.md</span>, use <span className="ww-mono">⌘B</span> and{' '}
        <span className="ww-mono">⌘D</span>, double-tap <span className="ww-mono">⇧</span> for commands.
      </p>
    </div>
  );
}

function HexPane(): ReactNode {
  const rows = genHex();
  return (
    <div className="wwx-hex codescroll">
      <div className="wwx-hex-inner">
        {rows.map((r, k) => (
          <div className="wwx-hex-row" key={k}>
            <span className="wwx-hex-off">{r.off}</span>
            <span className="wwx-hex-bytes">{r.left + '  ' + r.right}</span>
            <span className="wwx-hex-ascii">{'|' + r.ascii + '|'}</span>
          </div>
        ))}
      </div>
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
  return escapeHtml(pre) + '<span class="wwx-mark">' + escapeHtml(mid) + '</span>' + escapeHtml(post);
}
