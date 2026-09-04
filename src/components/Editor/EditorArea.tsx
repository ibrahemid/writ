import { Show, createEffect, onCleanup } from "solid-js";
import StatusBar from "./StatusBar";
import TabBar from "./TabBar";
import Toolbar from "../Toolbar/Toolbar";
import WordCount from "./WordCount";
import FindOverlay from "../Find/FindOverlay";
import SpellingPreview from "./SpellingPreview";
import PreviewLayout from "../Preview/PreviewLayout";
import { useActiveBuffer } from "../../lib/use-active-buffer";
import { configStore } from "../../stores/global/config";
import { findStore } from "../../stores/global/find-store";
import "./EditorArea.css";

export default function EditorArea() {
  const activeBuffer = useActiveBuffer();
  const statusBarOn = () => configStore.config().editor.status_bar;

  // The toast stack is a sibling of this subtree, not a descendant, so the
  // clearance the status bar needs has to reach it through the root.
  createEffect(() => {
    document.documentElement.style.setProperty(
      "--writ-toast-bottom",
      statusBarOn() ? "40px" : "16px",
    );
  });
  onCleanup(() => document.documentElement.style.removeProperty("--writ-toast-bottom"));

  // Find targets the active buffer's text; once it closes there is nothing
  // left to search, so the panel should not linger open over an empty canvas.
  createEffect(() => {
    if (!activeBuffer()) findStore.close();
  });

  return (
    <div class="editor-area">
      <Toolbar />
      <TabBar />
      <div class="editor-content">
        {/* Always mounted, even with no active buffer, so the preview iframe
            element it owns is never torn down (#124 webview freeze). */}
        <PreviewLayout buffer={activeBuffer()} />
        <Show when={!statusBarOn() && activeBuffer()}>
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
