import { Show } from "solid-js";
import { workspaceStore } from "../../stores/global/workspace";
import { basename } from "../../lib/path";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import FileTree from "./FileTree";

export default function FilesSection() {
  return (
    <Show when={workspaceStore.root()}>
      {(root) => (
        <div class="sidebar-section files-section">
          <div class="files-section-head">
            <div class="sidebar-section-title">{basename(root())}</div>
            <Tooltip label="Close folder">
              <button
                type="button"
                class="sidebar-section-action files-section-action"
                aria-label="Close folder"
                onClick={() => void workspaceStore.closeFolder()}
              >
                <Icon name="x" size={14} />
              </button>
            </Tooltip>
          </div>
          <FileTree />
        </div>
      )}
    </Show>
  );
}
