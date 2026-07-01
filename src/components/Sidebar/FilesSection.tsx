import { Show } from "solid-js";
import { workspaceStore } from "../../stores/global/workspace";
import { basename } from "../../lib/path";
import FileTree from "./FileTree";

export default function FilesSection() {
  return (
    <Show when={workspaceStore.root()}>
      {(root) => (
        <div class="sidebar-section files-section">
          <div class="files-section-head">
            <div class="sidebar-section-title" title={root()}>
              {basename(root())}
            </div>
            <button
              type="button"
              class="files-section-action"
              aria-label="Close folder"
              onClick={() => void workspaceStore.closeFolder()}
            >
              ✕
            </button>
          </div>
          <FileTree />
        </div>
      )}
    </Show>
  );
}
