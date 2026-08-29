import { Show } from "solid-js";
import StatusBar from "./StatusBar";
import Toolbar from "../Toolbar/Toolbar";
import WordCount from "./WordCount";
import FindOverlay from "../Find/FindOverlay";
import SpellingPreview from "./SpellingPreview";
import PreviewLayout from "../Preview/PreviewLayout";
import { useActiveBuffer } from "../../lib/use-active-buffer";
import { configStore } from "../../stores/global/config";
import "./EditorArea.css";

export default function EditorArea() {
  const activeBuffer = useActiveBuffer();
  const statusBarOn = () => configStore.config().editor.status_bar;

  return (
    <div class="editor-area">
      <Toolbar />
      <div class="editor-content">
        {/* Always mounted, even with no active buffer, so the preview iframe
            element it owns is never torn down (#124 webview freeze). */}
        <PreviewLayout buffer={activeBuffer()} />
        <Show when={!statusBarOn()}>
          <WordCount class="editor-wordcount" />
        </Show>
        <FindOverlay />
        <SpellingPreview />
      </div>
      <Show when={statusBarOn()}>
        <StatusBar />
      </Show>
    </div>
  );
}
