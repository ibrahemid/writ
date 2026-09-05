import { EditorState, StateEffect, type Extension } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  ViewUpdate,
  type DecorationSet,
  type PluginValue,
} from "@codemirror/view";
import { findLinkTargets, mergeLinkRanges, type LinkRange } from "./link-layer";

/** Whether a `[[…]]` target names one note, several, or none. */
export type WikilinkStatus = "resolved" | "ambiguous" | "missing";

/**
 * What the layer needs to paint a wikilink, injected so this file reaches no
 * store and no service: it knows the shape of a link and nothing about where
 * notes live.
 */
export interface WikilinkDeps {
  /** The note being edited, as the resolver keys it. Null for one with no file. */
  fromPath(): string | null;
  /**
   * What is known about `target` right now. A target nothing has resolved yet
   * reads as `null` and is painted as neither resolved nor missing, so a link
   * never flashes the wrong state while its answer is in flight.
   */
  known(fromPath: string, target: string): WikilinkStatus | null;
  /** Asks for `target`; the promise settles once [`known`] can answer. */
  resolve(fromPath: string, target: string): Promise<void>;
  /**
   * How many times the cache behind [`known`] has been emptied.
   *
   * The layer remembers which targets it has already asked about, which is what
   * stops a cache that cannot answer from being asked once per keystroke. A
   * number that moved means that record is stale, so a link whose note has just
   * appeared is asked about again rather than staying painted as missing.
   */
  generation(): number;
}

/** Class every wikilink carries, whatever it resolved to. */
export const WIKILINK_CLASS = "cm-md-wikilink";
/** Added to a target that names exactly one note. */
export const WIKILINK_RESOLVED_CLASS = "cm-md-wikilink-resolved";
/** Added to a target that names no note, or more than one. */
export const WIKILINK_MISSING_CLASS = "cm-md-wikilink-missing";

const baseMark = Decoration.mark({ class: WIKILINK_CLASS });
const resolvedMark = Decoration.mark({
  class: `${WIKILINK_CLASS} ${WIKILINK_RESOLVED_CLASS}`,
});
const missingMark = Decoration.mark({
  class: `${WIKILINK_CLASS} ${WIKILINK_MISSING_CLASS}`,
});

/** Dispatched when a resolution lands, so the plugin repaints. */
const wikilinksResolved = StateEffect.define<null>();

/**
 * The marks for the wikilinks among `ranges`.
 *
 * Kept apart from the plugin so the mapping from a status to a class is one
 * pure function a test can drive with any answer.
 */
export function wikilinkDecorations(
  state: EditorState,
  ranges: readonly LinkRange[],
  statusOf: (target: string) => WikilinkStatus | null,
): DecorationSet {
  const marks = [];
  for (const range of ranges) {
    if (range.kind !== "wikilink") continue;
    const status = statusOf(state.doc.sliceString(range.from, range.to));
    const mark =
      status === "resolved" ? resolvedMark : status === null ? baseMark : missingMark;
    marks.push(mark.range(range.from, range.to));
  }
  return Decoration.set(marks);
}

function wikilinkRanges(view: EditorView): LinkRange[] {
  const found: LinkRange[] = [];
  for (const { from, to } of view.visibleRanges) {
    found.push(...findLinkTargets(view.state, from, to));
  }
  return mergeLinkRanges(found).filter((range) => range.kind === "wikilink");
}

class WikilinkView implements PluginValue {
  decorations: DecorationSet;
  private ranges: LinkRange[];
  private asked = new Set<string>();
  private generation: number;
  private destroyed = false;

  constructor(
    private view: EditorView,
    private deps: WikilinkDeps,
  ) {
    this.ranges = wikilinkRanges(view);
    this.generation = deps.generation();
    this.decorations = this.build();
    this.ask();
  }

  update(update: ViewUpdate) {
    const landed = update.transactions.some((tr) =>
      tr.effects.some((effect) => effect.is(wikilinksResolved)),
    );
    const emptied = this.deps.generation() !== this.generation;
    if (!update.docChanged && !update.viewportChanged && !landed && !emptied) return;
    this.ranges = wikilinkRanges(update.view);
    this.decorations = this.build();
    if (!landed) this.ask();
  }

  destroy() {
    this.destroyed = true;
  }

  private build(): DecorationSet {
    const from = this.deps.fromPath();
    return wikilinkDecorations(this.view.state, this.ranges, (target) =>
      from === null ? "missing" : this.deps.known(from, target),
    );
  }

  /**
   * Reads whatever the painted links do not have an answer for yet, then
   * repaints once for the whole batch.
   *
   * A target is asked about once per generation: the cache drops what it holds
   * when a note changes on disk and says so through the generation, and every
   * target is then asked about again.
   */
  private ask(): void {
    const from = this.deps.fromPath();
    if (from === null) return;
    const generation = this.deps.generation();
    if (generation !== this.generation) {
      this.generation = generation;
      this.asked.clear();
    }
    const wanted = new Set<string>();
    for (const range of this.ranges) {
      const target = this.view.state.doc.sliceString(range.from, range.to);
      if (this.deps.known(from, target) !== null) continue;
      if (this.asked.has(target)) continue;
      wanted.add(target);
    }
    if (wanted.size === 0) return;
    for (const target of wanted) this.asked.add(target);
    void Promise.all([...wanted].map((target) => this.deps.resolve(from, target))).then(
      () => {
        if (this.destroyed) return;
        this.view.dispatch({ effects: wikilinksResolved.of(null) });
      },
      () => undefined,
    );
  }
}

/**
 * Paints `[[…]]` by whether the note it names is there.
 *
 * Resolution is asynchronous and decoration is not, so the layer paints from
 * what the injected cache already holds and repaints when an answer lands.
 */
export function wikilinkDecorationLayer(deps: WikilinkDeps): Extension {
  return ViewPlugin.define((view) => new WikilinkView(view, deps), {
    decorations: (value) => value.decorations,
  });
}
