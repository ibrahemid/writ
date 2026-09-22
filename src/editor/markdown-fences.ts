import {
  StateEffect,
  StateField,
  type EditorState,
  type Extension,
} from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  type DecorationSet,
  type PluginValue,
  type ViewUpdate,
} from "@codemirror/view";
import { syntaxTree } from "@codemirror/language";

// Minimal structural types matching @lezer/common, mirroring the shape used
// by editor/markdown-typography.ts so this module keeps out of the direct
// dependency list.
interface SyntaxNodeLike {
  readonly name: string;
  readonly from: number;
  readonly to: number;
  readonly firstChild: SyntaxNodeLike | null;
  readonly nextSibling: SyntaxNodeLike | null;
}

interface DocLine {
  readonly from: number;
  readonly to: number;
}

/** The two fence lines of one fenced block, ready to be taken off screen. */
export interface FenceLines {
  /** Line holding the opening fence and its info string. */
  open: DocLine;
  /** Line holding the closing fence. */
  close: DocLine;
}

// Block containers a fenced block can be written inside. Everything else is
// inline and cannot hold one, so the scan stops there rather than walking
// every emphasis mark in the document.
const BLOCK_CONTAINERS = new Set([
  "Document",
  "Blockquote",
  "BulletList",
  "OrderedList",
  "ListItem",
  "FencedCode",
]);

/**
 * The fence lines of a block whose fences may be taken off screen, or null.
 *
 * Null when the block never closed, when it is one line, or when it opens the
 * document: hiding a line means swallowing the break in front of it, and the
 * first line of a document has none. The caller then leaves the fences where
 * they are, dimmed, which is what shipped before.
 */
export function fenceCollapsible(
  node: SyntaxNodeLike,
  docLineAt: (pos: number) => DocLine,
): FenceLines | null {
  let marks = 0;
  let last: SyntaxNodeLike | null = null;
  for (let child = node.firstChild; child; child = child.nextSibling) {
    if (child.name !== "CodeMark") continue;
    marks++;
    last = child;
  }
  if (marks < 2 || !last) return null;

  let open: DocLine;
  let close: DocLine;
  try {
    open = docLineAt(node.from);
    close = docLineAt(last.from);
  } catch {
    return null;
  }
  if (open.from === 0) return null;
  if (open.from >= close.from) return null;
  return { open, close };
}

/** The ranges that take one block's fence lines off screen. */
export function fenceReplaceRanges(fences: FenceLines): { from: number; to: number }[] {
  // The range starts on the break in front of the line, so the line is
  // removed rather than left empty.
  return [
    { from: fences.open.from - 1, to: fences.open.to },
    { from: fences.close.from - 1, to: fences.close.to },
  ];
}

/**
 * Every collapsible fenced block in `[from, to)`.
 *
 * The walk descends into block containers only: an inline node cannot hold a
 * fenced block, so a document of prose costs one step per paragraph rather
 * than one per emphasis mark.
 */
export function collapsibleFences(
  iterateTree: (
    from: number,
    to: number,
    cb: (node: { name: string; node: SyntaxNodeLike }) => boolean | void,
  ) => void,
  docLineAt: (pos: number) => DocLine,
  from: number,
  to: number,
): FenceLines[] {
  const found: FenceLines[] = [];
  iterateTree(from, to, (node) => {
    if (node.name === "FencedCode") {
      const fences = fenceCollapsible(node.node, docLineAt);
      if (fences) found.push(fences);
      return false;
    }
    return BLOCK_CONTAINERS.has(node.name);
  });
  return found;
}

/** The stretch of the document the fences are known for. */
export interface FenceWindow {
  from: number;
  to: number;
}

/**
 * Widens the window the fences are scanned for.
 *
 * Dispatched by the companion view plugin: a state field cannot see the
 * viewport, and scanning a whole 2 MB document on every keystroke costs more
 * than a frame.
 */
export const setFenceWindow = StateEffect.define<FenceWindow>();

function fencesOf(state: EditorState, window: FenceWindow): FenceLines[] {
  const tree = syntaxTree(state);
  return collapsibleFences(
    (from, to, cb) => tree.iterate({ from, to, enter: cb }),
    (pos) => state.doc.lineAt(pos),
    window.from,
    window.to,
  );
}

const fenceReplace = Decoration.replace({});

interface FenceState {
  window: FenceWindow;
  fences: FenceLines[];
  decorations: DecorationSet;
}

function decorationsFor(
  fences: FenceLines[],
  selection: { ranges: readonly { from: number; to: number }[] },
): DecorationSet {
  const ranges: { from: number; to: number }[] = [];
  for (const block of fences) {
    // A block reveals both of its fences at once: editing the closing line
    // while the opening one is hidden reads as a block with one end.
    const touched = selection.ranges.some(
      (range) =>
        (range.from <= block.open.to && range.to >= block.open.from) ||
        (range.from <= block.close.to && range.to >= block.close.from),
    );
    if (touched) continue;
    ranges.push(...fenceReplaceRanges(block));
  }
  return Decoration.set(ranges.map((r) => fenceReplace.range(r.from, r.to)));
}

/**
 * Takes the fence lines of a closed fenced block off the screen.
 *
 * A state field rather than a part of the markdown view plugin, because a
 * decoration that replaces a line break may not come from a plugin. The
 * viewport reaches it as an effect instead: scanning a 2 MB document on
 * every keystroke costs more than a frame, and only what is on screen has a
 * fence to hide.
 *
 * Until the first window arrives the field scans what the parser has
 * reached, which on a large document is its opening and on a small one is
 * all of it.
 */
export const fenceCollapseField: StateField<FenceState> = StateField.define<FenceState>({
  create(state) {
    const window = { from: 0, to: Math.min(syntaxTree(state).length, state.doc.length) };
    const fences = fencesOf(state, window);
    return { window, fences, decorations: decorationsFor(fences, state.selection) };
  },
  update(value, tr) {
    let window = value.window;
    for (const effect of tr.effects) {
      if (effect.is(setFenceWindow)) window = effect.value;
    }
    const moved = window !== value.window;
    if (moved || tr.docChanged || syntaxTree(tr.startState) !== syntaxTree(tr.state)) {
      if (!moved) {
        // The window's positions are the old document's; a change under them
        // moves the text they name.
        window = {
          from: tr.changes.mapPos(window.from, -1),
          to: tr.changes.mapPos(window.to, 1),
        };
      }
      const fences = fencesOf(tr.state, window);
      return { window, fences, decorations: decorationsFor(fences, tr.state.selection) };
    }
    if (!tr.startState.selection.eq(tr.state.selection)) {
      return {
        window,
        fences: value.fences,
        decorations: decorationsFor(value.fences, tr.state.selection),
      };
    }
    return value;
  },
  provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
});

/**
 * Tells the field which stretch of the document is on screen.
 *
 * Dispatched after the update it reads, because CodeMirror refuses a
 * dispatch from inside one, and only when the window actually moved, so the
 * pair cannot feed each other.
 */
class FenceWindowPlugin implements PluginValue {
  private destroyed = false;

  constructor(private readonly view: EditorView) {
    this.publish();
  }

  update(update: ViewUpdate) {
    if (update.viewportChanged || update.docChanged) this.publish();
  }

  destroy() {
    this.destroyed = true;
  }

  private publish() {
    const ranges = this.view.visibleRanges;
    if (ranges.length === 0) return;
    const next = { from: ranges[0].from, to: ranges[ranges.length - 1].to };
    const held = this.view.state.field(fenceCollapseField, false)?.window;
    if (held && held.from === next.from && held.to === next.to) return;
    queueMicrotask(() => {
      if (this.destroyed) return;
      this.view.dispatch({ effects: setFenceWindow.of(next) });
    });
  }
}

/** The fence collapse: the field that hides them and the window it reads. */
export const fenceCollapse: Extension = [
  fenceCollapseField,
  ViewPlugin.define((view) => new FenceWindowPlugin(view)),
];
