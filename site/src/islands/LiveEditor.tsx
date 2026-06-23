import { useEffect, useRef } from 'react';
import { EditorState } from '@codemirror/state';
import { EditorView, lineNumbers, drawSelection, keymap } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { markdown } from '@codemirror/lang-markdown';

// Wasm renderer — this module is only imported once; init() is idempotent.
import initWasm, { render_fragment } from '../wasm/writ-render/writ_render.js';

const SEED = `# Retrieval audit

Recall improved after the \`rerank\` stage, mostly on long-tail queries.

> Hit-rate is bounded by the index recall ceiling.

## Flow
\`\`\`mermaid
graph LR
  Q[Query] --> E[Embed]
  E --> R[Retrieve]
  R --> K[Rerank]
\`\`\`

## Precision
$$ P = \\frac{tp}{tp + fp} $$`;

const KATEX_DELIMITERS = [
  { left: '$$', right: '$$', display: true },
  { left: '$', right: '$', display: false },
];

/** Mermaid themeVariables mirroring mockup index.html:746-747 */
function mermaidThemeVars(isDark: boolean) {
  return {
    primaryColor: isDark ? '#1a1a2e' : '#eef1f7',
    primaryBorderColor: isDark ? '#8aa6ff' : '#3b5bdb',
    primaryTextColor: isDark ? '#e0e0e0' : '#1a1a22',
    lineColor: '#8a8aa0',
    fontSize: '12px',
  };
}

let mermaidDiagramId = 0;

/**
 * Core render→post-process pipeline. Exported for unit testing.
 * Called after wasm init() has already resolved.
 */
export async function runRenderPipeline(
  text: string,
  docEl: HTMLElement,
): Promise<void> {
  const fragment = render_fragment(text) as {
    html: string;
    has_mermaid: boolean;
    has_math: boolean;
  };

  docEl.innerHTML = fragment.html;

  if (fragment.has_mermaid) {
    // Parity with the Writ app: app ships mermaid@11.15.0 (src-tauri/assets/mermaid/) which
    // embeds its own KaTeX, AND a standalone katex@0.16.22 (src-tauri/assets/katex/). This
    // site mirrors that exactly — do NOT dedupe the transitive mermaid→katex via pnpm overrides.
    const { default: mermaid } = await import('mermaid');
    const isDark =
      document.documentElement.getAttribute('data-theme') === 'dark';
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: 'strict',
      theme: 'base',
      fontFamily: 'Inter, sans-serif',
      themeVariables: mermaidThemeVars(isDark),
    });
    const nodes = Array.from(
      docEl.querySelectorAll<HTMLElement>('pre.mermaid'),
    );
    for (const node of nodes) {
      node.id = node.id || `writ-m-${mermaidDiagramId++}`;
    }
    try {
      await mermaid.run({ nodes });
    } catch {
      // Non-fatal: leave the raw text if the diagram is malformed
    }
  }

  if (fragment.has_math) {
    const [{ default: renderMathInElement }] = await Promise.all([
      import('katex/dist/contrib/auto-render'),
      import('katex/dist/katex.min.css'),
    ]);
    try {
      renderMathInElement(docEl, {
        delimiters: KATEX_DELIMITERS,
        throwOnError: false,
      });
    } catch {
      // Non-fatal
    }
  }
}

/** Initialise wasm once across the page lifetime. */
let wasmReady: Promise<void> | null = null;
function ensureWasm(): Promise<void> {
  if (!wasmReady) {
    wasmReady = initWasm().then(() => undefined);
  }
  return wasmReady;
}

export default function LiveEditor() {
  const editorRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    // Guard against double-mount (React strict mode)
    if (viewRef.current) return;

    // This island intentionally controls the SSR-rendered preview siblings in the hero layout
    // (#doc = preview pane, #cc/#lc = status bar, #srchint = search hint).
    const docEl = document.getElementById('doc') as HTMLElement | null;
    const ccEl = document.getElementById('cc') as HTMLElement | null;
    const lcEl = document.getElementById('lc') as HTMLElement | null;
    const hintEl = document.getElementById('srchint') as HTMLElement | null;

    function updateStatusBar(view: EditorView) {
      const state = view.state;
      const len = state.doc.length;
      if (ccEl) {
        ccEl.textContent =
          '≈ ' + Math.max(1, Math.round(len / 4)).toLocaleString() + ' tokens';
      }
      if (lcEl) {
        const sel = state.selection.main;
        const line = state.doc.lineAt(sel.head);
        lcEl.textContent = `Ln ${line.number}, Col ${sel.head - line.from + 1}`;
      }
    }

    async function renderAndPost(text: string) {
      if (!docEl) return;
      try {
        await ensureWasm();
        await runRenderPipeline(text, docEl);
      } catch {
        // Keep existing preview on failure
      }
    }

    function scheduleRender(view: EditorView) {
      if (debounceRef.current) clearTimeout(debounceRef.current);
      debounceRef.current = setTimeout(() => {
        const text = view.state.doc.toString();
        void renderAndPost(text);
        updateStatusBar(view);
      }, 90);
    }

    const updateListener = EditorView.updateListener.of((update) => {
      if (update.docChanged || update.selectionSet) {
        if (hintEl) hintEl.style.opacity = '0';
        scheduleRender(update.view);
      }
    });

    const theme = EditorView.theme({
      '&': {
        fontFamily: "'JetBrains Mono', monospace",
        fontSize: '12.5px',
        height: '100%',
        background: 'transparent',
      },
      '.cm-content': { caretColor: 'var(--accent, #3b5bdb)', padding: '0' },
      '.cm-line': { padding: '0' },
      '.cm-scroller': { overflow: 'auto', lineHeight: '1.6' },
      '.cm-focused': { outline: 'none' },
      '.cm-cursor': { borderLeftColor: 'var(--accent, #3b5bdb)' },
    });

    const state = EditorState.create({
      doc: SEED,
      extensions: [
        history(),
        drawSelection(),
        lineNumbers(),
        markdown(),
        EditorView.lineWrapping,
        keymap.of([...defaultKeymap, ...historyKeymap]),
        updateListener,
        theme,
      ],
    });

    if (!editorRef.current) return;

    const view = new EditorView({ state, parent: editorRef.current });
    viewRef.current = view;

    // Initial render (wasm will be awaited internally)
    scheduleRender(view);

    // Re-theme Mermaid on theme toggle (Phase H dispatches 'writ:theme';
    // also watch data-theme attribute directly for completeness)
    function onThemeChange() {
      const text = view.state.doc.toString();
      void renderAndPost(text);
    }
    document.addEventListener('writ:theme', onThemeChange);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
      document.removeEventListener('writ:theme', onThemeChange);
      view.destroy();
      viewRef.current = null;
    };
  }, []);

  return <div ref={editorRef} className="cm-host" aria-label="Markdown source, editable" />;
}
