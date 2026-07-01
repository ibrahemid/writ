import StatusBar from "./StatusBar";
import FindOverlay from "../Find/FindOverlay";
import PreviewLayout from "../Preview/PreviewLayout";
import { useActiveBuffer } from "../../lib/use-active-buffer";
import "./EditorArea.css";

export default function EditorArea() {
  const activeBuffer = useActiveBuffer();

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
