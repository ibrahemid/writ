import { For, Show, createMemo, onMount } from "solid-js";
import Kbd from "../Kbd/Kbd";
import { useWindow } from "../WindowProvider/WindowProvider";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { executeCommand } from "../../commands/registry";
import { commandBinding } from "../../lib/command-chord";
import { displayFileName } from "../../lib/display-name";
import { basename } from "../../lib/path";
import { tabFolderHints } from "../../lib/tab-labels";
import type { BufferDocument } from "../../types/buffer";
import "./StartSurface.css";

export const START_RECENT_LIMIT = 5;

const START_ACTIONS = [
  { commandId: "note.new", label: "New file" },
  { commandId: "file.open", label: "Open file…" },
  { commandId: "note.today", label: "Today's file" },
  { commandId: "notes.quickOpen", label: "Open file by name" },
  { commandId: "palette.open", label: "Command palette" },
] as const;

function closedAt(doc: BufferDocument): string {
  return doc.closed_at ?? doc.updated_at ?? doc.created_at;
}

export function recentFiles(history: readonly BufferDocument[], limit = START_RECENT_LIMIT): BufferDocument[] {
  return history
    .filter((doc) => doc.source_path)
    .slice()
    .sort((a, b) => closedAt(b).localeCompare(closedAt(a)))
    .slice(0, limit);
}

export default function StartSurface() {
  const win = useWindow();
  const rows: HTMLButtonElement[] = [];
  const recent = createMemo(() => recentFiles(bufferRegistry.historyList()));
  const recentHints = createMemo(() => tabFolderHints(recent()));

  onMount(() => {
    const active = document.activeElement;
    if (active === null || active === document.body) rows[0]?.focus();
  });

  function onKeyDown(e: KeyboardEvent) {
    const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (step === 0) return;
    const live = rows.filter((row) => row.isConnected);
    const at = live.indexOf(e.target as HTMLButtonElement);
    if (at === -1) return;
    e.preventDefault();
    live[(at + step + live.length) % live.length]?.focus();
  }

  return (
    <div class="editor-empty start-surface" onKeyDown={onKeyDown}>
      <div class="start-surface-list">
        <For each={START_ACTIONS}>
          {(action, index) => {
            const binding = () => commandBinding(action.commandId);
            return (
              <button
                ref={(el) => (rows[index()] = el)}
                type="button"
                class="start-surface-row"
                onClick={() => void executeCommand(action.commandId)}
              >
                <span class="start-surface-label">{action.label}</span>
                <Show when={binding()}>{(chord) => <Kbd binding={chord()} muted />}</Show>
              </button>
            );
          }}
        </For>
      </div>
      <Show when={recent().length > 0}>
        <div class="start-surface-list" role="group" aria-labelledby="start-surface-recent">
          <p id="start-surface-recent" class="start-surface-heading">
            Recent
          </p>
          <For each={recent()}>
            {(doc, index) => {
              const hint = () => recentHints().get(doc.id);
              return (
                <button
                  ref={(el) => (rows[START_ACTIONS.length + index()] = el)}
                  type="button"
                  class="start-surface-row"
                  onClick={() => void win.tabs.restoreFromHistory(doc.id)}
                >
                  <span class="start-surface-label">
                    {displayFileName(basename(doc.source_path ?? doc.title))}
                  </span>
                  <Show when={hint()}>
                    {(folder) => <span class="start-surface-folder">{folder()}</span>}
                  </Show>
                </button>
              );
            }}
          </For>
        </div>
      </Show>
    </div>
  );
}
