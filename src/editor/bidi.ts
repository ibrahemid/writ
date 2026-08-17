import { RangeSetBuilder, type Extension } from "@codemirror/state";
import {
  EditorView,
  ViewPlugin,
  ViewUpdate,
  Decoration,
  type DecorationSet,
  type PluginValue,
} from "@codemirror/view";

// `EditorView.perLineTextDirection` only makes CodeMirror *read* each rendered
// line's direction (via getComputedStyle on the line element); it never assigns
// one. Auto direction therefore has to come from a `dir="auto"` attribute on
// every line, and the HTML auto-directionality algorithm cannot be expressed in
// CSS, so the attribute is attached with line decorations over the viewport.
const autoDirLine = Decoration.line({ attributes: { dir: "auto" } });

/**
 * Builds one `dir="auto"` line decoration per rendered line.
 *
 * Only visible ranges are walked, so cost scales with the viewport rather than
 * the document. A line straddling two visible ranges is decorated once.
 */
export function buildAutoDirectionDecorations(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const { doc } = view.state;
  let lastLineFrom = -1;
  for (const { from, to } of view.visibleRanges) {
    for (let pos = from; pos <= to; ) {
      const line = doc.lineAt(pos);
      if (line.from > lastLineFrom) {
        builder.add(line.from, line.from, autoDirLine);
        lastLineFrom = line.from;
      }
      pos = line.to + 1;
    }
  }
  return builder.finish();
}

class AutoDirectionPlugin implements PluginValue {
  decorations: DecorationSet;

  constructor(view: EditorView) {
    this.decorations = buildAutoDirectionDecorations(view);
  }

  update(update: ViewUpdate) {
    if (update.docChanged || update.viewportChanged) {
      this.decorations = buildAutoDirectionDecorations(update.view);
    }
  }
}

/**
 * Per-line automatic text direction.
 *
 * Each line picks left-to-right or right-to-left from its own first strong
 * directional character, so Arabic and Latin lines coexist in one document.
 * The gutter is outside the content element and keeps its left placement.
 */
export const autoTextDirection: Extension = [
  EditorView.perLineTextDirection.of(true),
  ViewPlugin.fromClass(AutoDirectionPlugin, { decorations: (v) => v.decorations }),
];
