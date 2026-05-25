import { Show, createMemo } from "solid-js";
import EditorInstance from "./EditorInstance";
import StatusBar from "./StatusBar";
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
          {(buf) => <EditorInstance buffer={buf()} />}
        </Show>
      </div>
      <StatusBar />
    </div>
  );
}
