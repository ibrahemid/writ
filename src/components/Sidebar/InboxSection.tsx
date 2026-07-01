import { For, Show } from "solid-js";
import { inboxStore } from "../../stores/global/inbox";
import { useWindow } from "../WindowProvider/WindowProvider";
import { formatBytes } from "../../lib/format-bytes";
import { basename } from "../../lib/path";
import "./InboxSection.css";

export default function InboxSection() {
  const win = useWindow();

  return (
    <Show when={inboxStore.path()}>
      {(root) => (
        <div class="sidebar-section inbox-section">
          <div class="inbox-section-head">
            <div class="sidebar-section-title" title={root()}>
              Inbox · {basename(root())}
            </div>
            <button
              type="button"
              class="inbox-section-action"
              aria-label="Stop watching folder"
              onClick={() => void inboxStore.stopWatching()}
            >
              ✕
            </button>
          </div>
          <Show
            when={inboxStore.files().length > 0}
            fallback={<div class="inbox-empty">No files yet</div>}
          >
            <div class="inbox-list">
              <For each={inboxStore.files()}>
                {(file) => (
                  <button
                    type="button"
                    class="inbox-item"
                    title={file.path}
                    onClick={() => void win.tabs.openFile(file.path)}
                  >
                    <span class="inbox-item-name">{file.name}</span>
                    <span class="inbox-item-size">{formatBytes(file.size_bytes)}</span>
                  </button>
                )}
              </For>
            </div>
          </Show>
        </div>
      )}
    </Show>
  );
}
