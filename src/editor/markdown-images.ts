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
  | { status: "ready"; dataUrl: string; label: string }
  | { status: "failed"; label: string; reason: InlineImageFailure };

/**
 * How a reference becomes bytes.
 *
 * Injected, so this module reaches no service and no store and the editor
 * layer stays testable without an IPC mock.
 */
export interface InlineImageDeps {
  resolve(reference: string): Promise<{ dataUrl: string }>;
}

/** Records what is known about one reference. */
export const setInlineImage = StateEffect.define<{
  reference: string;
  state: InlineImageState;
}>();

const NO_IMAGES: ReadonlyMap<string, InlineImageState> = new Map();

/**
 * Bytes already resolved, keyed by the reference as authored.
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
  too_large: "over 2 MB",
  not_an_image: "not an image",
};

/**
 * The picture, drawn under the line that authored it.
 *
 * An inline widget whose container is a block box, not a block decoration:
 * CodeMirror refuses a block decoration from a view plugin, and the markdown
 * decorations are viewport-scoped, which a state field cannot be.
 *
 * The container reserves its height before the bytes arrive, so the `ready`
 * transition does not move the lines under it.
 */
export class InlineImageWidget extends WidgetType {
  constructor(
    readonly reference: string,
    readonly state: InlineImageState,
  ) {
    super();
  }

  override eq(other: InlineImageWidget): boolean {
    if (other.reference !== this.reference) return false;
    if (other.state.status !== this.state.status) return false;
    return other.state.status === "ready" && this.state.status === "ready"
      ? other.state.dataUrl === this.state.dataUrl
      : true;
  }

  toDOM(): HTMLElement {
    const frame = document.createElement("div");
    frame.className = "cm-md-image";
    frame.dataset.status = this.state.status;
    if (this.state.status === "ready") {
      const image = document.createElement("img");
      image.src = this.state.dataUrl;
      image.alt = this.state.label;
      frame.appendChild(image);
    } else if (this.state.status === "failed") {
      const caption = document.createElement("span");
      caption.className = "cm-md-image-caption";
      caption.textContent = `${this.state.label} · ${FAILURE_LABELS[this.state.reason]}`;
      frame.appendChild(caption);
    }
    return frame;
  }

  override ignoreEvent(): boolean {
    return false;
  }
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
 */
class InlineImageRequester implements PluginValue {
  private readonly asked = new Set<string>();
  private destroyed = false;

  constructor(
    private readonly view: EditorView,
    private readonly deps: InlineImageDeps,
  ) {
    this.scan();
  }

  update(update: ViewUpdate) {
    // The tree fills in asynchronously for a long document, so a parse that
    // finishes without an edit still reveals images the scan has not seen.
    if (
      update.docChanged ||
      update.viewportChanged ||
      syntaxTree(update.startState) !== syntaxTree(update.state)
    ) {
      this.scan();
    }
  }

  destroy() {
    this.destroyed = true;
  }

  private scan() {
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
        ({ dataUrl }) => this.settle(reference, { status: "ready", dataUrl, label }),
        (error: unknown) =>
          this.settle(reference, { status: "failed", label, reason: failureOf(error) }),
      );
    }
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
