import { Show } from "solid-js";
import { workspaceStore } from "../../stores/global/workspace";
import FileTree from "./FileTree";

function folderName(root: string): string {
  const cut = Math.max(root.lastIndexOf("/"), root.lastIndexOf("\\"));
  return cut >= 0 ? root.slice(cut + 1) || root : root;
}

export default function FilesSection() {
  return (
    <Show when={workspaceStore.root()}>
      {(root) => (
        <div class="sidebar-section files-section">
          <div class="files-section-head">
            <div class="sidebar-section-title" title={root()}>
              {folderName(root())}
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
