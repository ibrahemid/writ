import { editorStore } from "../../stores/editor";
import "./StatusBar.css";

export default function StatusBar() {
  return (
    <div class="statusbar">
      <div class="statusbar-item">
        {editorStore.language() ?? "Plain Text"}
      </div>
      <div class="statusbar-spacer" />
      <div class="statusbar-item">
        Ln {editorStore.cursorLine()}, Col {editorStore.cursorCol()}
        {editorStore.selectionCount() > 1 && ` (${editorStore.selectionCount()} cursors)`}
      </div>
      <div class="statusbar-item">
        {editorStore.lineCount()} lines
      </div>
      <div class="statusbar-item">UTF-8</div>
    </div>
  );
}
