import {
  autocompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import type { EditorView } from "@codemirror/view";
import type { Extension } from "@codemirror/state";
import { isInsideCode } from "./link-layer";

/** One note a completion can offer. */
export interface NoteName {
  path: string;
  /** The note's file name without the extension. */
  name: string;
}

/**
 * Where the names come from, injected so this file reaches no store and no
 * service.
 */
export interface WikilinkCompleteDeps {
  candidates(query: string): Promise<NoteName[]>;
}

/**
 * The text between an open `[[` and the cursor, or null when the cursor is
 * not inside one.
 *
 * Only the line the cursor is on is read: a wikilink does not span lines, and
 * a scan bounded by the line cannot be made expensive by a long document. A
 * `]]` between the brackets and the cursor closes the link, so a cursor after
 * a finished one completes nothing.
 */
export function wikilinkQueryAt(line: string, column: number): string | null {
  const before = line.slice(0, column);
  const open = before.lastIndexOf("[[");
  if (open === -1) return null;
  const inner = before.slice(open + 2);
  if (inner.includes("]]") || inner.includes("[")) return null;
  return inner;
}

/**
 * Completes a `[[` from the note names the index holds.
 *
 * The completion inserts the note's name, not its path: a name is what a
 * wikilink is written with, and the resolver takes it from there.
 */
export function wikilinkCompletionSource(deps: WikilinkCompleteDeps) {
  return async (context: CompletionContext): Promise<CompletionResult | null> => {
    const line = context.state.doc.lineAt(context.pos);
    const query = wikilinkQueryAt(line.text, context.pos - line.from);
    if (query === null) return null;
    // A wikilink written inside code is an example of the syntax. Offering
    // note names there puts a list over documentation of the link itself.
    if (isInsideCode(context.state, context.pos)) return null;
    // An empty `[[` opens the list only when the user asked for it, so typing
    // the brackets does not put a panel over what is being written.
    if (query.trim() === "" && !context.explicit) return null;

    const hits = await deps.candidates(query);
    if (hits.length === 0) return null;
    return {
      from: context.pos - query.length,
      to: context.pos,
      options: hits.map((hit) => ({
        label: hit.name,
        detail: hit.path,
        type: "text",
        apply: applyName,
      })),
      // The list is ranked by the index; re-filtering here would reorder it.
      filter: false,
    };
  };
}

/**
 * Writes the chosen name and closes the link.
 *
 * `closeBrackets` pairs a typed `[[` and leaves the `]]` already written, but a
 * pasted or re-opened `[[` has none, and a completion that inserted only the
 * name would leave the link unclosed. The close is written when it is not
 * already there, and the caret lands after it either way.
 */
function applyName(view: EditorView, completion: Completion, from: number, to: number): void {
  const closed = view.state.doc.sliceString(to, Math.min(to + 2, view.state.doc.length)) === "]]";
  const insert = closed ? completion.label : `${completion.label}]]`;
  const caret = from + insert.length + (closed ? 2 : 0);
  view.dispatch({
    changes: { from, to, insert },
    selection: { anchor: caret },
    userEvent: "input.complete",
  });
}

/** The `[[` completion, as an editor extension. */
export function wikilinkCompletion(deps: WikilinkCompleteDeps): Extension {
  return autocompletion({
    override: [wikilinkCompletionSource(deps)],
    // A note name is prose, so nothing is inserted until it is chosen.
    defaultKeymap: true,
    activateOnTyping: true,
  });
}
