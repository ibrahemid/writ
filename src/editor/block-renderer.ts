import mermaidRuntimeUrl from "mermaid/dist/mermaid.min.js?url";
import { logFailure } from "../lib/log";
import { typesetMath, type MathAnswer } from "./math-render";

/** Which of the two diagram themes a drawing is made in. */
export type BlockTheme = "light" | "dark";

/**
 * What draws the blocks Inline renders: KaTeX for math and Mermaid for
 * diagrams, the runtimes and versions the preview ships. An answer is the
 * renderer's markup as it came out: it reaches the page only through
 * `sanitizedFragment` (editor/sanitize-markup.ts).
 */
export interface BlockRenderer {
  renderMath(tex: string, display: boolean): Promise<string>;
  renderDiagram(source: string, theme: BlockTheme): Promise<string>;
}

/** A block that could not be drawn. `line` is the diagram line an error names. */
export class BlockRenderError extends Error {
  constructor(
    readonly kind: "math" | "diagram",
    message: string,
    readonly line: number | null = null,
  ) {
    super(message);
    this.name = "BlockRenderError";
  }
}

// ─── Math ──────────────────────────────────────────────────────────────────

let mathStyles: Promise<unknown> | null = null;

/** KaTeX's stylesheet and fonts, fetched the first time math is drawn. */
function loadMathStyles(): void {
  mathStyles ??= import("katex/dist/katex.min.css").catch(() => {
    mathStyles = null;
    logFailure("the math stylesheet could not be loaded");
  });
}

async function typesetOnPage(tex: string, display: boolean): Promise<string> {
  const katex = (await import("katex")).default;
  return typesetMath(katex, tex, display);
}

interface MathJob {
  tex: string;
  display: boolean;
  resolve(markup: string): void;
  reject(error: unknown): void;
}

/**
 * Typesets in a worker, so a screen of math costs the page nothing while it
 * is drawn. A webview that cannot start the worker, or a worker that fails,
 * hands its work to the page.
 */
class MathTypesetter {
  private worker: Worker | null | undefined;
  private nextId = 0;
  private readonly waiting = new Map<number, MathJob>();

  typeset(tex: string, display: boolean): Promise<string> {
    loadMathStyles();
    const worker = this.worker === undefined ? this.start() : this.worker;
    if (worker === null) return typesetOnPage(tex, display);
    return new Promise<string>((resolve, reject) => {
      const id = ++this.nextId;
      this.waiting.set(id, { tex, display, resolve, reject });
      worker.postMessage({ id, tex, display });
    });
  }

  private start(): Worker | null {
    if (typeof Worker === "undefined") {
      this.worker = null;
      return null;
    }
    try {
      const worker = new Worker(new URL("./math.worker.ts", import.meta.url));
      worker.onmessage = (event: MessageEvent<MathAnswer>) => this.answer(event.data);
      worker.onerror = () => this.fallBack();
      this.worker = worker;
    } catch {
      logFailure("math is typeset on the page: the worker could not start");
      this.worker = null;
    }
    return this.worker;
  }

  private answer(answer: MathAnswer): void {
    const job = this.waiting.get(answer.id);
    if (!job) return;
    this.waiting.delete(answer.id);
    if ("markup" in answer) job.resolve(answer.markup);
    else job.reject(new BlockRenderError("math", answer.error));
  }

  private fallBack(): void {
    logFailure("math is typeset on the page: the worker stopped");
    this.worker?.terminate();
    this.worker = null;
    const jobs = [...this.waiting.values()];
    this.waiting.clear();
    for (const job of jobs) typesetOnPage(job.tex, job.display).then(job.resolve, job.reject);
  }
}

// ─── Diagrams ──────────────────────────────────────────────────────────────

interface MermaidApi {
  initialize(config: Record<string, unknown>): void;
  parse(text: string): Promise<unknown>;
  render(id: string, text: string): Promise<{ svg: string }>;
}

function pageMermaid(): MermaidApi | undefined {
  return (window as unknown as { mermaid?: MermaidApi }).mermaid;
}

function nextTask(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function errorLine(error: unknown): number | null {
  const message = error instanceof Error ? error.message : String(error);
  const match = /\bline (\d+)/i.exec(message);
  return match ? Number(match[1]) : null;
}

/**
 * Draws diagrams one at a time, each in a task of its own, so a document of
 * diagrams never holds the page for longer than one of them takes. Mermaid
 * lays a diagram out by measuring text in the document, which a worker does
 * not have: this is the one part of a block's drawing that runs on the
 * page's thread.
 *
 * The runtime is the file the preview loads (`mermaid.min.js`), fetched the
 * first time a diagram is drawn, and set up as the preview sets it up:
 * `securityLevel: "strict"` and a theme for each polarity.
 */
class DiagramDrawer {
  private runtime: Promise<MermaidApi> | null = null;
  private queue: Promise<unknown> = Promise.resolve();
  private theme: BlockTheme | null = null;
  private drawn = 0;

  draw(source: string, theme: BlockTheme): Promise<string> {
    const job = this.queue.then(nextTask).then(() => this.drawNow(source, theme));
    this.queue = job.catch(() => undefined);
    return job;
  }

  private async drawNow(source: string, theme: BlockTheme): Promise<string> {
    const mermaid = await this.load();
    if (this.theme !== theme) {
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: "strict",
        // Mermaid draws into the live document before the sanitizer sees the
        // SVG, so a directive in the file must not be able to set CSS there.
        secure: ["secure", "securityLevel", "startOnLoad", "maxTextSize", "suppressErrorRendering", "maxEdges", "themeCSS", "themeVariables", "fontFamily"],
        theme: theme === "dark" ? "dark" : "default",
      });
      this.theme = theme;
    }
    const id = `writ-diagram-${++this.drawn}`;
    try {
      await mermaid.parse(source);
      const { svg } = await mermaid.render(id, source);
      return svg;
    } catch (error) {
      throw new BlockRenderError(
        "diagram",
        error instanceof Error ? error.message : String(error),
        errorLine(error),
      );
    } finally {
      // A failed render can leave its scratch element in the page.
      document.getElementById(`d${id}`)?.remove();
      document.getElementById(id)?.remove();
    }
  }

  private load(): Promise<MermaidApi> {
    this.runtime ??= new Promise<MermaidApi>((resolve, reject) => {
      const present = pageMermaid();
      if (present) {
        resolve(present);
        return;
      }
      const script = document.createElement("script");
      script.src = mermaidRuntimeUrl;
      script.async = true;
      script.addEventListener("load", () => {
        const loaded = pageMermaid();
        if (loaded) resolve(loaded);
        else reject(new BlockRenderError("diagram", "the diagram runtime loaded without its API"));
      });
      script.addEventListener("error", () =>
        reject(new BlockRenderError("diagram", "the diagram runtime did not load")),
      );
      document.head.appendChild(script);
    }).catch((error: unknown) => {
      this.runtime = null;
      logFailure("the diagram runtime could not be loaded");
      throw error;
    });
    return this.runtime;
  }
}

// ─── The renderer ──────────────────────────────────────────────────────────

/** How many drawings are kept, across every tab. */
const KEPT_DRAWINGS = 256;

/**
 * A renderer whose answers are kept by source: the same expression or
 * diagram in another tab, or after an undo, is drawn once. A failure is not
 * kept, so the next request draws again.
 */
export function createBlockRenderer(): BlockRenderer {
  const math = new MathTypesetter();
  const diagrams = new DiagramDrawer();
  const kept = new Map<string, Promise<string>>();

  function keep(key: string, draw: () => Promise<string>): Promise<string> {
    const held = kept.get(key);
    if (held) {
      kept.delete(key);
      kept.set(key, held);
      return held;
    }
    const drawing = draw();
    kept.set(key, drawing);
    drawing.catch(() => kept.delete(key));
    if (kept.size > KEPT_DRAWINGS) {
      const oldest = kept.keys().next().value;
      if (oldest !== undefined) kept.delete(oldest);
    }
    return drawing;
  }

  return {
    renderMath: (tex, display) =>
      keep(`math\u0000${display ? "d" : "i"}\u0000${tex}`, () => math.typeset(tex, display)),
    renderDiagram: (source, theme) =>
      keep(`diagram\u0000${theme}\u0000${source}`, () => diagrams.draw(source, theme)),
  };
}

let shared: BlockRenderer | null = null;

/** The renderer every editor in the window shares. */
export function sharedBlockRenderer(): BlockRenderer {
  shared ??= createBlockRenderer();
  return shared;
}
