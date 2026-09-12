import { For, Show } from "solid-js";
import { inboxStore } from "../../stores/global/inbox";
import { useWindow } from "../WindowProvider/WindowProvider";
import { formatBytes } from "../../lib/format-bytes";
import { basename } from "../../lib/path";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import SidebarSection from "./SidebarSection";
import "./InboxSection.css";

// "Inbox · Inbox" says nothing twice; the folder name only earns its place
// when it differs from the feature's own name.
export function inboxTitle(folder: string): string {
  return folder.trim().toLowerCase() === "inbox" ? "Inbox" : `Inbox · ${folder}`;
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
          heading={inboxTitle(basename(root()))}
          class="inbox-section"
          action={
            <Tooltip label="Stop watching folder">
              <button
                type="button"
                class="sidebar-section-action inbox-section-action"
                aria-label="Stop watching folder"
                onClick={() => void inboxStore.stopWatching()}
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
                    class="inbox-item"
                    onClick={() => void win.tabs.openFile(file.path)}
                  >
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
