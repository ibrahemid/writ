import { For, Show } from "solid-js";
import { inboxStore } from "../../stores/global/inbox";
import { useWindow } from "../WindowProvider/WindowProvider";
import { formatBytes } from "../../lib/format-bytes";
import { basename } from "../../lib/path";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import "./InboxSection.css";

// "Inbox · Inbox" says nothing twice; the folder name only earns its place
// when it differs from the feature's own name.
export function inboxTitle(folder: string): string {
  return folder.trim().toLowerCase() === "inbox" ? "Inbox" : `Inbox · ${folder}`;
}

export default function InboxSection() {
  const win = useWindow();

  return (
    <Show when={inboxStore.path()}>
      {(root) => (
        <div class="sidebar-section inbox-section">
          <div class="inbox-section-head">
            <div class="sidebar-section-title">{inboxTitle(basename(root()))}</div>
            <Tooltip label="Stop watching folder">
              <button
                type="button"
                class="sidebar-section-action inbox-section-action"
                aria-label="Stop watching folder"
                onClick={() => void inboxStore.stopWatching()}
              >
                <Icon name="x" size={14} />
              </button>
            </Tooltip>
          </div>
          <Show
            when={inboxStore.files().length > 0}
            fallback={<div class="inbox-empty">No files yet</div>}
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
          </Show>
        </div>
      )}
    </Show>
  );
}
