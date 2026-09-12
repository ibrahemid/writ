import { Show } from "solid-js";
import { workspaceStore } from "../../stores/global/workspace";
import { basename } from "../../lib/path";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import FileTree from "./FileTree";
import SidebarSection from "./SidebarSection";

export default function FilesSection() {
  return (
    <Show when={workspaceStore.root()}>
      {(root) => (
        <SidebarSection
          id="folder"
          heading={basename(root())}
          class="files-section"
          action={
            <Tooltip label="Close folder">
              <button
                type="button"
                class="sidebar-section-action files-section-action"
                aria-label="Close folder"
                onClick={() => void workspaceStore.closeFolder()}
              >
                <Icon name="x" size={16} />
              </button>
            </Tooltip>
          }
        >
          <FileTree />
        </SidebarSection>
      )}
    </Show>
  );
}
