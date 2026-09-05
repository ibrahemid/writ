import {
  EditorState,
  Prec,
  StateEffect,
  StateField,
  type Extension,
} from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  ViewUpdate,
  keymap,
  type DecorationSet,
  type PluginValue,
} from "@codemirror/view";
import { syntaxTree } from "@codemirror/language";
import { IS_MAC } from "../lib/platform";

// Minimal structural type matching @lezer/common, mirroring the shape used by
// editor/markdown-typography.ts so this module keeps out of the direct
// dependency list.
interface SyntaxNodeRef {
  readonly name: string;
  readonly from: number;
  readonly to: number;
  readonly node: { readonly parent: { readonly name: string } | null };
}

export type LinkKind = "url" | "path" | "wikilink";

export interface LinkRange {
  from: number;
  to: number;
  kind: LinkKind;
}

// The layer never reaches a service or a store. `EditorInstance` injects both
// actions, so the policy (what an external link means, what counts as inside
// the workspace) stays in the layers that own it and this file stays DOM-pure
// and testable without an IPC mock.
export interface LinkDeps {
  openUrl(url: string): void;
  openWorkspaceFile(path: string): void;
  /**
   * What to do with a `[[…]]` target: open the note it names, offer to create
   * it, or show the notes it could mean. The layer knows the shape of a
   * wikilink and nothing about what one resolves to, which is why this is
   * injected rather than read from a store.
   */
  openNoteLink?(target: string): boolean;
}

// Anchored on a known scheme and length-bounded so a pathological line cannot
// turn decoration into a scan of the whole buffer.
const URL_RUN = /(?:https?:\/\/|mailto:)[^\s<>"'`\\]{1,2048}/g;
// The inside of a `[[…]]`, length-bounded for the same reason as the URL run.
// A nested `[` or `]` ends the candidate, so `[[a]b]]` is not a target and an
// unclosed `[[` cannot swallow the rest of the line.
const WIKILINK_RUN = /\[\[([^\[\]\n]{1,512})\]\]/g;
// Two or more characters before the colon: a single letter is a Windows drive
// (`C:\notes`), never a scheme.
const HAS_SCHEME = /^[a-zA-Z][a-zA-Z0-9+.-]+:/;
const IDENT_CHAR = /[\p{L}\p{N}_]/u;
const TRAILING_PUNCTUATION = ".,;:!?*_~'\"";
const CLOSERS: Record<string, string> = { ")": "(", "]": "[", "}": "{" };

const linkMark = Decoration.mark({ class: "writ-link" });

const setModifier = StateEffect.define<boolean>();

// Styling only. A click reads the modifier off its own event, because this
// field is stale whenever the modifier went down while the editor was not
// focused.
const modifierField = StateField.define<boolean>({
  create: () => false,
  update(value, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setModifier)) value = effect.value;
    }
    return value;
  },
});

export function modifierIsHeld(state: EditorState): boolean {
  return state.field(modifierField, false) ?? false;
}

export function isLinkModifier(event: { metaKey: boolean; ctrlKey: boolean }): boolean {
  return IS_MAC ? event.metaKey : event.ctrlKey;
}

function countChar(text: string, char: string): number {
  let n = 0;
  for (const c of text) if (c === char) n++;
  return n;
}

// Trailing punctuation belongs to the prose, not the address. A closing
// bracket is kept only when the run opened one, so `(see https://x)` and
// `[label](https://x)` both stop before the bracket while
// `https://x/wiki/Foo_(bar)` keeps it.
export function trimUrlTail(text: string): string {
  let end = text.length;
  while (end > 0) {
    const char = text[end - 1];
    if (TRAILING_PUNCTUATION.includes(char)) {
      end--;
      continue;
    }
    const opener = CLOSERS[char];
    if (opener !== undefined) {
      const slice = text.slice(0, end);
      if (countChar(slice, char) > countChar(slice, opener)) {
        end--;
        continue;
      }
    }
    break;
  }
  return text.slice(0, end);
}

function kindOf(destination: string): LinkKind {
  return HAS_SCHEME.test(destination) ? "url" : "path";
}

// Sorted, non-overlapping, zero-length ranges dropped — the shape
// `Decoration.set` requires. Earlier entries win an overlap, so the caller
// controls precedence by insertion order.
export function mergeLinkRanges(ranges: LinkRange[]): LinkRange[] {
  const sorted = [...ranges].sort((a, b) => a.from - b.from);
  const out: LinkRange[] = [];
  let reach = -1;
  for (const range of sorted) {
    if (range.to <= range.from) continue;
    if (range.from < reach) continue;
    out.push(range);
    reach = range.to;
  }
  return out;
}

// Link runs inside `[from, to)`, widened to whole lines so a URL straddling a
// viewport edge is found in one piece rather than as two half-addresses.
export function findLinkTargets(state: EditorState, from: number, to: number): LinkRange[] {
  const docLength = state.doc.length;
  const start = state.doc.lineAt(Math.max(0, Math.min(from, docLength))).from;
  const end = state.doc.lineAt(Math.max(0, Math.min(to, docLength))).to;
  if (end <= start) return [];

  const found: LinkRange[] = [];
  const text = state.doc.sliceString(start, end);

  // Wikilinks first, so `[[https://x]]` is one target rather than an address
  // with brackets around it, and `[[Note]]` is not read as a shortcut
  // reference link.
  WIKILINK_RUN.lastIndex = 0;
  for (
    let match = WIKILINK_RUN.exec(text);
    match !== null;
    match = WIKILINK_RUN.exec(text)
  ) {
    // `![[…]]` is an embed the preview resolves as a file, not a note link.
    if (match.index > 0 && text[match.index - 1] === "!") continue;
    if (match[1].trim() === "") continue;
    const inner = start + match.index + 2;
    found.push({ from: inner, to: inner + match[1].length, kind: "wikilink" });
  }

  // Markdown destinations next, so a `[label](https://x)` destination wins the
  // overlap against the bare run inside it.
  syntaxTree(state).iterate({
    from: start,
    to: end,
    enter: (node: SyntaxNodeRef) => {
      if (node.name !== "URL") return;
      if (node.node.parent?.name === "Image") return;
      let { from: nodeFrom, to: nodeTo } = node;
      let text = state.doc.sliceString(nodeFrom, nodeTo);
      // A pointy-bracket destination, `[label](<a b.md>)`, carries its
      // delimiters in the node.
      if (text.length > 1 && text.startsWith("<") && text.endsWith(">")) {
        nodeFrom++;
        nodeTo--;
        text = text.slice(1, -1);
      }
      found.push({ from: nodeFrom, to: nodeTo, kind: kindOf(text) });
    },
  });

  URL_RUN.lastIndex = 0;
  for (let match = URL_RUN.exec(text); match !== null; match = URL_RUN.exec(text)) {
    const before = match.index > 0 ? text[match.index - 1] : "";
    if (before !== "" && IDENT_CHAR.test(before)) continue;
    const run = trimUrlTail(match[0]);
    if (run.length === 0) continue;
    found.push({ from: start + match.index, to: start + match.index + run.length, kind: "url" });
  }

  return mergeLinkRanges(found);
}

// The click decision, isolated from the DOM so it can be driven directly.
// `pos` is null when the pointer is outside the content.
export function linkClickTarget(
  ranges: readonly LinkRange[],
  pos: number | null,
  modifierHeld: boolean,
  button: number,
): LinkRange | null {
  if (button !== 0 || !modifierHeld || pos === null) return null;
  for (const range of ranges) {
    if (pos >= range.from && pos < range.to) return range;
  }
  return null;
}

function rangesForView(view: EditorView): LinkRange[] {
  const found: LinkRange[] = [];
  for (const { from, to } of view.visibleRanges) {
    found.push(...findLinkTargets(view.state, from, to));
  }
  return mergeLinkRanges(found);
}

class LinkView implements PluginValue {
  ranges: LinkRange[];
  decorations: DecorationSet;

  constructor(view: EditorView) {
    this.ranges = rangesForView(view);
    this.decorations = this.build();
  }

  update(update: ViewUpdate) {
    // The syntax tree fills in asynchronously for a long document, so a parse
    // that finishes without an edit still has to repaint markdown destinations.
    if (
      update.docChanged ||
      update.viewportChanged ||
      syntaxTree(update.startState) !== syntaxTree(update.state)
    ) {
      this.ranges = rangesForView(update.view);
      this.decorations = this.build();
    }
  }

  private build(): DecorationSet {
    // A wikilink is painted by `wikilink-decorations`, which knows whether the
    // note it names is there. Marking it here would say every target is a
    // destination.
    return Decoration.set(
      this.ranges
        .filter((r) => r.kind !== "wikilink")
        .map((r) => linkMark.range(r.from, r.to)),
    );
  }
}

const linkPlugin = ViewPlugin.fromClass(LinkView, { decorations: (v) => v.decorations });

/**
 * The `[[…]]` target the caret sits inside, or null.
 *
 * The caret has to be strictly inside the target text: at either edge the
 * bracket pair has only just been typed or is about to be closed, and Enter
 * there is a line break rather than a destination.
 */
export function wikilinkAtCursor(state: EditorState): string | null {
  const selection = state.selection.main;
  if (!selection.empty) return null;
  const pos = selection.head;
  const line = state.doc.lineAt(pos);
  for (const range of findLinkTargets(state, line.from, line.to)) {
    if (range.kind !== "wikilink") continue;
    if (pos > range.from && pos < range.to) {
      return state.doc.sliceString(range.from, range.to);
    }
  }
  return null;
}

function syncModifier(view: EditorView, next: boolean): void {
  if (modifierIsHeld(view.state) === next) return;
  view.dispatch({ effects: setModifier.of(next) });
}

export function linkLayer(deps: LinkDeps): Extension {
  return [
    modifierField,
    linkPlugin,
    // Above the newline binding, which claims every Enter, and below the
    // completion panel's own Enter, registered at the highest precedence, so
    // accepting a name from the `[[` list still wins. Handing the keystroke
    // back is what keeps Enter a line break in a note the link cannot be
    // followed from.
    Prec.high(
      keymap.of([
        {
          key: "Enter",
          run: (view) => {
            const target = wikilinkAtCursor(view.state);
            if (target === null || !deps.openNoteLink) return false;
            return deps.openNoteLink(target);
          },
        },
      ]),
    ),
    EditorView.editorAttributes.compute([modifierField], (state) =>
      state.field(modifierField, false)
        ? { class: "writ-link-active" }
        : ({} as Record<string, string>),
    ),
    EditorView.domEventHandlers({
      // Mouse selection starts on mousedown, so this is the hook that can take
      // the gesture away from it. Returning true on a link is the precedence
      // rule against CodeMirror's own modifier-click add-cursor: on a link the
      // click opens and adds no cursor, everywhere else add-cursor is
      // untouched.
      mousedown(event, view) {
        const instance = view.plugin(linkPlugin);
        if (!instance) return false;
        const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
        const hit = linkClickTarget(instance.ranges, pos, isLinkModifier(event), event.button);
        if (!hit) return false;
        event.preventDefault();
        const target = view.state.doc.sliceString(hit.from, hit.to);
        if (hit.kind === "url") {
          deps.openUrl(target);
        } else if (hit.kind === "wikilink") {
          deps.openNoteLink?.(target);
        } else {
          deps.openWorkspaceFile(target);
        }
        return true;
      },
      keydown(event, view) {
        syncModifier(view, isLinkModifier(event));
        return false;
      },
      keyup(event, view) {
        syncModifier(view, isLinkModifier(event));
        return false;
      },
      // A modifier released outside the window never reaches keyup, which
      // would otherwise leave the editor stuck showing links as clickable.
      blur(_event, view) {
        syncModifier(view, false);
        return false;
      },
    }),
  ];
}
