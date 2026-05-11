import { Show, createMemo } from "solid-js";
import EditorInstance from "./EditorInstance";
import StatusBar from "./StatusBar";
import { bufferStore } from "../../stores/buffers";
import "./EditorArea.css";

export default function EditorArea() {
  const activeBuffer = createMemo(() => {
    const id = bufferStore.activeTabId();
    if (!id) return null;
    return bufferStore.activeTabs().find(b => b.id === id) ?? null;
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
