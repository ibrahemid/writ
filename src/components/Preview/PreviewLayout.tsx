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
  buffer: BufferDocument;
}

export default function PreviewLayout(props: Props) {
  const win = useWindow();
  let containerEl: HTMLDivElement | undefined;
  // id → renderable-at-resolution. Resolve a buffer's layout on first sight
  // and again only on the non-renderable → renderable transition (#122: a
  // scratch buffer renamed .txt → .md). A buffer resolved while renderable
  // is never re-resolved, so a user's manual layout choice is never clobbered.
  const resolvedRenderable = new Map<string, boolean>();

  const contentType = createMemo(() => contentTypeForBuffer(props.buffer));
  const renderable = createMemo(() => rendererRegistry.hasRenderer(contentType()));
  const layout = createMemo<LayoutMode>(() => win.layout.get(props.buffer.id));

  // Resolve initial layout: persisted (source-backed) → content-type config
  // default (if renderable) → source. Re-runs on rename so a buffer that
  // becomes renderable picks up its content-type default without a reopen.
  createEffect(() => {
    const buf = props.buffer;
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
    const current = layout();
    if (current.kind === "split") {
      win.layout.setLocal(props.buffer.id, { ...current, ratio });
    }
  }

  function commitRatio() {
    win.layout.set(props.buffer.id, props.buffer.source_path, layout());
  }

  const previewIntent = () => {
    const k = layout().kind;
    return k === "split" || k === "preview";
  };
  // The ONLY path that mounts <PreviewPane> (and therefore an iframe): a
  // preview-intent layout on a buffer with a registered renderer.
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
        <EditorInstance buffer={props.buffer} />
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

      <Show when={showsIframe()}>
        <div class="preview-pane-slot">
          <PreviewPane buffer={props.buffer} contentType={contentType()!} />
        </div>
      </Show>

      <Show when={showsUnsupportedNote()}>
        <div class="preview-pane-slot preview-unsupported">
          <span>No preview available for this file type.</span>
        </div>
      </Show>
    </div>
  );
}
