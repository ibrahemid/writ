import { Show, createEffect, createMemo, type JSX } from "solid-js";
import type { BufferDocument } from "../../types/buffer";
import EditorInstance from "../Editor/EditorInstance";
import PreviewPane from "./PreviewPane";
import PreviewSplit from "./PreviewSplit";
import { configStore } from "../../stores/global/config";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import { contentTypeForBuffer } from "../../lib/content-type";
import { defaultSplit, type LayoutMode } from "../../lib/preview-layout";
import "./preview-chrome.css";

interface Props {
  // Nullable: the layout is always mounted (so the preview iframe element is
  // never torn down — see PreviewPane), even between buffers / when none is
  // open. A null buffer renders the empty state and parks the preview blank.
  buffer: BufferDocument | null;
}

const SOURCE_LAYOUT: LayoutMode = { kind: "source" };

export default function PreviewLayout(props: Props) {
  const win = useWindow();
  let containerEl: HTMLDivElement | undefined;
  // id → renderable-at-resolution. Resolve a buffer's layout on first sight
  // and again only on the non-renderable → renderable transition (#122: a
  // scratch buffer renamed .txt → .md). A buffer resolved while renderable
  // is never re-resolved, so a user's manual layout choice is never clobbered.
  const resolvedRenderable = new Map<string, boolean>();

  const contentType = createMemo(() =>
    props.buffer ? contentTypeForBuffer(props.buffer) : null,
  );
  const renderable = createMemo(() => {
    const ct = contentType();
    return ct !== null && rendererRegistry.hasRenderer(ct);
  });
  const layout = createMemo<LayoutMode>(() =>
    props.buffer ? win.layout.get(props.buffer.id) : SOURCE_LAYOUT,
  );

  // Resolve initial layout: persisted (source-backed) → content-type config
  // default (if renderable) → source. Re-runs on rename so a buffer that
  // becomes renderable picks up its content-type default without a reopen.
  createEffect(() => {
    const buf = props.buffer;
    if (!buf) return;
    const canRender = rendererRegistry.hasRenderer(contentTypeForBuffer(buf));
    const prev = resolvedRenderable.get(buf.id);
    if (prev === undefined || (prev === false && canRender)) {
      resolvedRenderable.set(buf.id, canRender);
      void initLayout(buf);
    }
  });

  async function initLayout(buf: BufferDocument) {
    const ct = contentTypeForBuffer(buf);
    if (!rendererRegistry.hasRenderer(ct)) {
      win.layout.setLocal(buf.id, { kind: "source" });
      return;
    }
    if (buf.source_path) {
      const persisted = await win.layout.hydrate(buf.source_path);
      if (persisted) {
        win.layout.setLocal(buf.id, persisted);
        return;
      }
    }
    const cfg = configStore.config().preview;
    const def =
      ct === "markdown" ? cfg.default_layout_markdown : cfg.default_layout_html;
    const resolved: LayoutMode =
      def === "split" ? defaultSplit() : def === "preview" ? { kind: "preview" } : { kind: "source" };
    win.layout.setLocal(buf.id, resolved);
  }

  function setRatioLive(ratio: number) {
    const buf = props.buffer;
    const current = layout();
    if (buf && current.kind === "split") {
      win.layout.setLocal(buf.id, { ...current, ratio });
    }
  }

  function commitRatio() {
    const buf = props.buffer;
    if (buf) win.layout.set(buf.id, buf.source_path, layout());
  }

  const previewIntent = () => {
    const k = layout().kind;
    return k === "split" || k === "preview";
  };
  // Whether a real preview is shown: a preview-intent layout on a renderable
  // buffer. Drives the persistent PreviewPane's `active` flag — when false the
  // iframe parks blank (it is never removed; teardown freezes the webview).
  const showsIframe = () => previewIntent() && renderable();
  // Recognized content type but no registered renderer: the user reached a
  // preview layout (e.g. via the cycle keymap, which doesn't renderer-check).
  // Show a friendly note in the pane slot — never a blank iframe.
  const showsUnsupportedNote = () =>
    previewIntent() && !renderable() && contentType() !== null;
  const isSplit = () => layout().kind === "split";
  const orientation = () => {
    const l = layout();
    return l.kind === "split" ? l.orientation : "vertical";
  };

  const editorStyle = (): JSX.CSSProperties => {
    const l = layout();
    // Hide the editor only when a real preview pane is showing. A
    // preview-intent layout on an unrenderable buffer keeps the editor
    // visible alongside the "no preview" note.
    if (l.kind === "preview" && showsIframe()) return { display: "none" };
    // Only reserve split width when a preview pane actually shows. A split
    // layout on an unrenderable buffer (e.g. .md renamed back to .txt) must
    // give the editor full width, not leave an empty pane gap.
    if (l.kind === "split" && renderable()) {
      return { "flex-grow": "0", "flex-shrink": "0", "flex-basis": `${l.ratio * 100}%` };
    }
    return { "flex-grow": "1", "flex-basis": "0" };
  };

  return (
    <div
      class="preview-layout"
      classList={{ "is-horizontal": orientation() === "horizontal" }}
      ref={containerEl}
    >
      <div class="preview-editor-slot" style={editorStyle()}>
        <Show
          when={props.buffer}
          fallback={<div class="editor-empty">No buffer open</div>}
        >
          {(buf) => <EditorInstance buffer={buf()} />}
        </Show>
      </div>

      <Show when={isSplit() && renderable()}>
        <PreviewSplit
          orientation={orientation()}
          ratio={layout().kind === "split" ? (layout() as { ratio: number }).ratio : 0.5}
          container={() => containerEl}
          onResize={setRatioLive}
          onCommit={commitRatio}
        />
      </Show>

      {/* The preview pane slot is ALWAYS in the DOM so its iframe is never torn
          down (writ-preview:// teardown freezes the macOS webview, #124). It is
          hidden when no preview shows; the pane parks the iframe on a blank doc. */}
      <div class="preview-pane-slot" classList={{ "is-hidden": !showsIframe() }}>
        <PreviewPane buffer={props.buffer} contentType={contentType()} isActive={showsIframe()} />
      </div>

      <Show when={showsUnsupportedNote()}>
        <div class="preview-pane-slot preview-unsupported">
          <span>No preview available for this file type.</span>
        </div>
      </Show>
    </div>
  );
}
