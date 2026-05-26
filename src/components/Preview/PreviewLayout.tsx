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
  const initialized = new Set<string>();

  const contentType = createMemo(() => contentTypeForBuffer(props.buffer));
  const renderable = createMemo(() => rendererRegistry.hasRenderer(contentType()));
  const layout = createMemo<LayoutMode>(() => win.layout.get(props.buffer.id));

  // Resolve a buffer's initial layout once: persisted (source-backed) →
  // content-type config default (if renderable) → source.
  createEffect(() => {
    const buf = props.buffer;
    if (initialized.has(buf.id)) return;
    initialized.add(buf.id);
    void initLayout(buf);
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

  const isSplit = () => layout().kind === "split";
  const showsPane = () => {
    const k = layout().kind;
    return (k === "split" || k === "preview") && renderable();
  };
  const orientation = () => {
    const l = layout();
    return l.kind === "split" ? l.orientation : "vertical";
  };

  const editorStyle = (): JSX.CSSProperties => {
    const l = layout();
    if (l.kind === "preview" && renderable()) return { display: "none" };
    if (l.kind === "split") {
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

      <Show when={showsPane()}>
        <div class="preview-pane-slot">
          <PreviewPane
            buffer={props.buffer}
            contentType={contentType()!}
            layout={layout().kind}
          />
        </div>
      </Show>
    </div>
  );
}
