import { Show, createEffect, createMemo, on } from "solid-js";
import { executeCommand } from "../../commands/registry";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import { previewScriptsStore } from "../../stores/global/preview-scripts";
import { useWindow } from "../WindowProvider/WindowProvider";
import { useActiveBuffer } from "../../lib/use-active-buffer";
import { contentTypeForBuffer } from "../../lib/content-type";
import "./PreviewScriptsField.css";

// Whether the previewed file's own scripts run, as its stored opt-in says
// (ADR-043). Shown while the preview shows an HTML or Markdown document; a
// click runs the same toggle as the command palette.

const DOCUMENT_TYPES = new Set(["html", "markdown"]);

export default function PreviewScriptsField() {
  const win = useWindow();
  const activeBuffer = useActiveBuffer();

  const previewedId = createMemo<string | null>(() => {
    const buf = activeBuffer();
    if (!buf) return null;
    const contentType = contentTypeForBuffer(buf);
    if (contentType === null || !DOCUMENT_TYPES.has(contentType)) return null;
    if (!rendererRegistry.hasRenderer(contentType)) return null;
    const kind = win.layout.get(buf.id, contentType).kind;
    if (kind !== "split" && kind !== "preview") return null;
    return win.editor.readFailure()?.bufferId === buf.id ? null : buf.id;
  });

  createEffect(
    on(previewedId, (id) => {
      if (id !== null) void previewScriptsStore.load(id);
    }),
  );

  const enabled = createMemo<boolean | undefined>(() => {
    const id = previewedId();
    return id === null ? undefined : previewScriptsStore.enabled(id);
  });

  return (
    <Show when={enabled() !== undefined}>
      <button
        type="button"
        class="statusbar-scripts"
        classList={{ "is-on": enabled() === true }}
        title="Toggle scripts for this file"
        onClick={() => executeCommand("preview.toggleRunScripts")}
      >
        {enabled() ? "Scripts on" : "Scripts off"}
      </button>
    </Show>
  );
}
