import { For, Show } from "solid-js";
import { inboxStore } from "../../stores/global/inbox";
import { useWindow } from "../WindowProvider/WindowProvider";
import { formatBytes } from "../../lib/format-bytes";
import { showToast } from "../../stores/global/notifications";
import { basename } from "../../lib/path";
import { logFailure } from "../../lib/log";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import SidebarSection from "./SidebarSection";
import "./InboxSection.css";

// A failed stop leaves the section on screen, so the click has to say it did
// nothing (NV-32).
function stopWatching() {
  inboxStore.stopWatching().catch(() => {
    logFailure("the watched folder could not be released");
    showToast("Could not stop watching the folder.", "error");
  });
}

// A watched folder with nothing in it yet is a setting, not a list, so the
// section waits for its first file the way every section waits for a row.
export default function InboxSection() {
  const win = useWindow();
  const watched = () => {
    const root = inboxStore.path();
    return root !== null && inboxStore.files().length > 0 ? root : null;
  };

  return (
    <Show when={watched()}>
      {(root) => (
        <SidebarSection
          id="inbox"
          heading={`Watched · ${basename(root())}`}
          class="inbox-section"
          action={
            <Tooltip label="Stop watching folder">
              <button
                type="button"
                class="sidebar-section-action inbox-section-action"
                aria-label="Stop watching folder"
                onClick={stopWatching}
              >
                <Icon name="x" size={16} />
              </button>
            </Tooltip>
          }
        >
          <div class="inbox-list">
            <For each={inboxStore.files()}>
              {(file) => (
                <Tooltip label={file.path}>
                  <button
                    type="button"
                    class="sidebar-row inbox-item"
                    onClick={() => void win.tabs.openFile(file.path)}
                  >
                    <Icon name="file-text" />
                    <span class="inbox-item-name">{file.name}</span>
                    <span class="inbox-item-size">{formatBytes(file.size_bytes)}</span>
                  </button>
                </Tooltip>
              )}
            </For>
          </div>
        </SidebarSection>
      )}
    </Show>
  );
}
