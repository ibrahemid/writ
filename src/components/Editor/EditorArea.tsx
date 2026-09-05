import { Match, Switch } from "solid-js";
import StatusBar from "./StatusBar";
import SaveFailureBar from "./SaveFailureBar";
import RemovedOnDiskBar from "./RemovedOnDiskBar";
import FileChangedBar from "./FileChangedBar";
import FindOverlay from "../Find/FindOverlay";
import SpellingPreview from "./SpellingPreview";
import PreviewLayout from "../Preview/PreviewLayout";
import { useActiveBuffer } from "../../lib/use-active-buffer";
import { useWindow } from "../WindowProvider/WindowProvider";
import "./EditorArea.css";

export default function EditorArea() {
  const activeBuffer = useActiveBuffer();
  const win = useWindow();

  // One file behind the note, one state, one bar. Mounted on that state rather
  // than side by side: a note that was both asked about and deleted would
  // otherwise carry two bars saying different things about one file, and the
  // question's answers read the file the deletion says is gone.
  const fileState = () => {
    const id = activeBuffer()?.id;
    return id === undefined ? "present" : win.editor.noteFileState(id);
  };

  return (
    <div class="editor-area">
      <Switch>
        <Match when={fileState() === "removed"}>
          <RemovedOnDiskBar noteId={activeBuffer()?.id ?? null} />
        </Match>
        <Match when={fileState() === "changed"}>
          <FileChangedBar noteId={activeBuffer()?.id ?? null} />
        </Match>
      </Switch>
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
