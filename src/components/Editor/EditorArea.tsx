import StatusBar from "./StatusBar";
import SaveFailureBar from "./SaveFailureBar";
import FindOverlay from "../Find/FindOverlay";
import SpellingPreview from "./SpellingPreview";
import PreviewLayout from "../Preview/PreviewLayout";
import { useActiveBuffer } from "../../lib/use-active-buffer";
import "./EditorArea.css";

export default function EditorArea() {
  const activeBuffer = useActiveBuffer();

  return (
    <div class="editor-area">
      <SaveFailureBar noteId={activeBuffer()?.id ?? null} />
      <div class="editor-content">
        {/* Always mounted, even with no active buffer, so the preview iframe
            element it owns is never torn down (#124 webview freeze). */}
        <PreviewLayout buffer={activeBuffer()} />
        <FindOverlay />
        <SpellingPreview />
      </div>
      <StatusBar />
    </div>
  );
}
