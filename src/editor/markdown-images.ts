import { StateEffect, StateField, type Extension } from "@codemirror/state";
import {
  EditorView,
  ViewPlugin,
  WidgetType,
  type PluginValue,
  type ViewUpdate,
} from "@codemirror/view";
import { syntaxTree } from "@codemirror/language";
import { isInlineImageFailure, type InlineImageFailure } from "../types/inline-image";

export type { InlineImageFailure };

// Minimal structural types matching @lezer/common, mirroring the shape used
// by editor/markdown-typography.ts so this module keeps out of the direct
// dependency list.
interface SyntaxNodeLike {
  readonly name: string;
  readonly from: number;
  readonly to: number;
  readonly firstChild: SyntaxNodeLike | null;
  readonly nextSibling: SyntaxNodeLike | null;
  readonly parent: SyntaxNodeLike | null;
}

/** What is known about one authored reference. */
export type InlineImageState =
  | { status: "pending" }
  | { status: "ready"; src: string; label: string }
  | { status: "downloading"; label: string }
  | { status: "failed"; label: string; reason: InlineImageFailure };

/**
 * How a reference becomes a URL the image is drawn from.
 *
 * Injected, so this module reaches no service and no store and the editor
 * layer stays testable without an IPC mock. `download` asks the sync provider
 * for an image it holds and answers once the bytes are here.
 */
export interface InlineImageDeps {
  resolve(reference: string): Promise<{ src: string }>;
  download(reference: string): Promise<{ src: string }>;
}

/** Records what is known about one reference. */
export const setInlineImage = StateEffect.define<{
  reference: string;
  state: InlineImageState;
}>();

/** Asks for the download of an image a sync provider holds. */
export const downloadInlineImage = StateEffect.define<string>();

/**
 * How long typing pauses before a reference it produced is asked for. Typing
 * inside `![](` makes a new reference of every prefix, and only the one left
 * when the typing stops names a file.
 */
export const TYPING_PAUSE_MS = 400;

const NO_IMAGES: ReadonlyMap<string, InlineImageState> = new Map();

/**
 * What each reference resolved to, keyed by the reference as authored.
 *
 * A field rather than plugin state: the round trip outlives any one update,
 * a reference repeated down the document costs one request, and the widget
 * re-reads what arrived without asking again.
 */
export const inlineImageField = StateField.define<ReadonlyMap<string, InlineImageState>>({
  create: () => NO_IMAGES,
  update(value, tr) {
    let next: Map<string, InlineImageState> | null = null;
    for (const effect of tr.effects) {
      if (!effect.is(setInlineImage)) continue;
      next ??= new Map(value);
      next.set(effect.value.reference, effect.value.state);
    }
    return next ?? value;
  },
});

/** The file name a reference ends in, which is what a caption names. */
export function imageLabel(reference: string): string {
  const cut = reference.replace(/\\/g, "/").split("/");
  return cut[cut.length - 1] || reference;
}

/**
 * The reference an `Image` node carries, read from its `URL` child.
 *
 * `node` may be the image itself or any node inside it, so a caller holding
 * a resolved position does not have to walk up first.
 */
export function imageReferenceAt(
  node: SyntaxNodeLike,
  docSlice: (from: number, to: number) => string,
): string | null {
  let image: SyntaxNodeLike | null = node;
  while (image && image.name !== "Image") image = image.parent;
  if (!image) return null;
  for (let child = image.firstChild; child; child = child.nextSibling) {
    if (child.name !== "URL") continue;
    const text = docSlice(child.from, child.to).trim();
    const inner = text.length > 1 && text.startsWith("<") && text.endsWith(">")
      ? text.slice(1, -1)
      : text;
    return inner.length > 0 ? inner : null;
  }
  return null;
}

const FAILURE_LABELS: Record<InlineImageFailure, string> = {
  outside_root: "outside the folder",
  not_found: "missing",
  not_downloaded: "not downloaded",
  too_large: "too large",
  not_an_image: "not an image",
};

function sameState(a: InlineImageState, b: InlineImageState): boolean {
  switch (a.status) {
    case "pending":
      return b.status === "pending";
    case "ready":
      return b.status === "ready" && a.src === b.src && a.label === b.label;
    case "downloading":
      return b.status === "downloading" && a.label === b.label;
    case "failed":
      return b.status === "failed" && a.reason === b.reason && a.label === b.label;
  }
}

/**
 * The picture, drawn under the line that authored it.
 *
 * An inline widget whose container is a block box, not a block decoration:
 * CodeMirror refuses a block decoration from a view plugin, and the markdown
 * decorations are viewport-scoped, which a state field cannot be.
 *
 * The container reserves its height before the bytes arrive, so the `ready`
 * transition does not move the lines under it. The bytes come from the
 * preview scheme, decoded off the main thread.
 */
export class InlineImageWidget extends WidgetType {
  constructor(
    readonly reference: string,
    readonly state: InlineImageState,
  ) {
    super();
  }

  override eq(other: InlineImageWidget): boolean {
    return other.reference === this.reference && sameState(other.state, this.state);
  }

  toDOM(view: EditorView): HTMLElement {
    const frame = document.createElement("div");
    frame.className = "cm-md-image";
    frame.dataset.status = this.state.status;
    if (this.state.status === "ready") {
      const image = document.createElement("img");
      image.decoding = "async";
      frame.dataset.loaded = "false";
      const loaded = () => {
        frame.dataset.loaded = "true";
      };
      image.addEventListener("load", loaded);
      image.addEventListener("error", loaded);
      image.src = this.state.src;
      image.alt = this.state.label;
      frame.appendChild(image);
    } else if (this.state.status === "downloading") {
      frame.appendChild(caption(`${this.state.label} · downloading`));
    } else if (this.state.status === "failed") {
      frame.appendChild(caption(`${this.state.label} · ${FAILURE_LABELS[this.state.reason]}`));
      if (this.state.reason === "not_downloaded") {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "writ-btn writ-btn-secondary cm-md-image-download";
        button.textContent = "Download";
        const reference = this.reference;
        button.addEventListener("click", () => {
          view.dispatch({ effects: downloadInlineImage.of(reference) });
        });
        frame.appendChild(button);
      }
    }
    return frame;
  }

  // The Download button takes its own click; everywhere else the editor
  // handles the event, so a click on the image places the cursor.
  override ignoreEvent(event: Event): boolean {
    return event.target instanceof Element && event.target.closest("button") !== null;
  }
}

function caption(text: string): HTMLElement {
  const span = document.createElement("span");
  span.className = "cm-md-image-caption";
  span.textContent = text;
  return span;
}

function failureOf(error: unknown): InlineImageFailure {
  const reason = (error as { reason?: unknown } | null)?.reason;
  return isInlineImageFailure(reason) ? reason : "not_found";
}

/**
 * Asks for every reference the viewport shows, once each.
 *
 * Emits no decorations: the widget itself is built by the markdown
 * decoration builder, which reads [`inlineImageField`]. Two decoration
 * sources over the same ranges would collide on replaced ranges neither can
 * see.
 *
 * While the document is being typed in, new references wait for a pause of
 * [`TYPING_PAUSE_MS`]: the scan that follows asks only for the ones still in
 * the text.
 */
class InlineImageRequester implements PluginValue {
  private readonly asked = new Set<string>();
  private readonly downloading = new Set<string>();
  private typingPause: ReturnType<typeof setTimeout> | null = null;
  private destroyed = false;

  constructor(
    private readonly view: EditorView,
    private readonly deps: InlineImageDeps,
  ) {
    this.scan();
  }

  update(update: ViewUpdate) {
    for (const transaction of update.transactions) {
      for (const effect of transaction.effects) {
        if (effect.is(downloadInlineImage)) this.download(effect.value);
      }
    }
    if (update.docChanged) {
      this.waitForTypingPause();
      return;
    }
    // The tree fills in asynchronously for a long document, so a parse that
    // finishes without an edit still reveals images the scan has not seen.
    if (update.viewportChanged || syntaxTree(update.startState) !== syntaxTree(update.state)) {
      this.scan();
    }
  }

  destroy() {
    this.destroyed = true;
    if (this.typingPause !== null) clearTimeout(this.typingPause);
  }

  private waitForTypingPause() {
    if (this.typingPause !== null) clearTimeout(this.typingPause);
    this.typingPause = setTimeout(() => {
      this.typingPause = null;
      if (!this.destroyed) this.scan();
    }, TYPING_PAUSE_MS);
  }

  private scan() {
    if (this.typingPause !== null) return;
    const { state } = this.view;
    const tree = syntaxTree(state);
    const slice = (from: number, to: number) => state.doc.sliceString(from, to);
    const fresh: string[] = [];
    for (const { from, to } of this.view.visibleRanges) {
      tree.iterate({
        from,
        to,
        enter: (node: { name: string; node: SyntaxNodeLike }) => {
          if (node.name !== "Image") return;
          const reference = imageReferenceAt(node.node, slice);
          if (!reference || this.asked.has(reference)) return;
          this.asked.add(reference);
          fresh.push(reference);
        },
      });
    }
    // Dispatching inside an update is refused by CodeMirror, and a scan runs
    // from one.
    if (fresh.length > 0) queueMicrotask(() => this.request(fresh));
  }

  private request(references: string[]) {
    if (this.destroyed) return;
    this.view.dispatch({
      effects: references.map((reference) =>
        setInlineImage.of({ reference, state: { status: "pending" } }),
      ),
    });
    for (const reference of references) {
      const label = imageLabel(reference);
      this.deps.resolve(reference).then(
        ({ src }) => this.settle(reference, { status: "ready", src, label }),
        (error: unknown) =>
          this.settle(reference, { status: "failed", label, reason: failureOf(error) }),
      );
    }
  }

  private download(reference: string) {
    if (this.downloading.has(reference)) return;
    this.downloading.add(reference);
    const label = imageLabel(reference);
    const done = () => this.downloading.delete(reference);
    // Called from an update, where CodeMirror refuses a dispatch.
    queueMicrotask(() => {
      this.settle(reference, { status: "downloading", label });
      this.deps.download(reference).then(
        ({ src }) => {
          done();
          this.settle(reference, { status: "ready", src, label });
        },
        (error: unknown) => {
          done();
          this.settle(reference, { status: "failed", label, reason: failureOf(error) });
        },
      );
    });
  }

  private settle(reference: string, state: InlineImageState) {
    if (this.destroyed) return;
    this.view.dispatch({ effects: setInlineImage.of({ reference, state }) });
  }
}

/** The cache and the requests behind the image widgets. */
export function inlineImages(deps: InlineImageDeps): Extension {
  return [
    inlineImageField,
    ViewPlugin.define((view) => new InlineImageRequester(view, deps)),
  ];
}

/** The states the decoration builder draws from, for a state that has them. */
export function inlineImageStates(state: {
  field: (field: typeof inlineImageField, require: false) => ReadonlyMap<string, InlineImageState> | undefined;
}): ReadonlyMap<string, InlineImageState> {
  return state.field(inlineImageField, false) ?? NO_IMAGES;
}
