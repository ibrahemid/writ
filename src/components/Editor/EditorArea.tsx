import { createMemo } from "solid-js";
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
        {/* Always mounted, even with no active buffer, so the preview iframe
            element it owns is never torn down (#124 webview freeze). */}
        <PreviewLayout buffer={activeBuffer()} />
        <FindOverlay />
      </div>
      <StatusBar />
    </div>
  );
}
