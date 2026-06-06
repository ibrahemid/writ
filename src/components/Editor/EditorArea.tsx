import { Show, createMemo } from "solid-js";
import StatusBar from "./StatusBar";
import FindOverlay from "../Find/FindOverlay";
import PreviewLayout from "../Preview/PreviewLayout";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import "./EditorArea.css";

export default function EditorArea() {
  const win = useWindow();
  const activeBuffer = createMemo(() => {
    const id = win.tabs.activeTabId();
    if (!id) return null;
    return bufferRegistry.activeTabs().find((b) => b.id === id) ?? null;
  });

  return (
    <div class="editor-area">
      <div class="editor-content">
        <Show when={activeBuffer()} fallback={<div class="editor-empty">No buffer open</div>}>
          {(buf) => <PreviewLayout buffer={buf()} />}
        </Show>
        <FindOverlay />
      </div>
      <StatusBar />
    </div>
  );
}
