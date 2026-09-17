import { createEffect, createSignal, For, onCleanup, Show, createUniqueId } from "solid-js";

import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
import { useWindow } from "../WindowProvider/WindowProvider";
import { installFocusTrap } from "../../lib/focus-trap";
import { showToast } from "../Notifications/Toast";
import { formatBytes } from "../../lib/format-bytes";
import { noteVersionsStore, type NoteVersion } from "../../stores/global/note-versions";
import "./NoteHistoryPanel.css";

// Singleton state — Writ is single-window

const [isOpen, setIsOpen] = createSignal(false);

export function openNoteVersions(path: string) {
  setIsOpen(true);
  void noteVersionsStore.load(path);
}

export function closeNoteVersions() {
  setIsOpen(false);
  noteVersionsStore.clear();
}

export function isNoteVersionsOpen(): boolean {
  return isOpen();
}

/** The note's name, which is what the header shows of its path. */
function nameOf(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/**
 * When a version was kept: the time today, the day and time yesterday, the
 * date and time before that, all in the reader's own locale.
 */
export function versionLabel(atMs: number, now: Date = new Date()): string {
  const stamp = new Date(atMs);
  if (Number.isNaN(stamp.getTime())) return "";
  const time = stamp.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  if (stamp.toDateString() === now.toDateString()) return `Today ${time}`;
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (stamp.toDateString() === yesterday.toDateString()) return `Yesterday ${time}`;
  return stamp.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function readableError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "That version was not written.";
}

function Row(props: { version: NoteVersion }) {
  const isSelected = () => noteVersionsStore.selected() === props.version.id;

  return (
    <li role="presentation">
      <button
        type="button"
        class="note-versions-row"
        role="option"
        data-version={props.version.id}
        aria-selected={isSelected()}
        tabindex={isSelected() ? 0 : -1}
        onClick={() => void noteVersionsStore.select(props.version.id)}
      >
        <span class="note-version-when">{versionLabel(props.version.at_ms)}</span>
        <span class="note-version-size">{formatBytes(props.version.bytes)}</span>
      </button>
    </li>
  );
}

function VersionsDialog() {
  const titleId = createUniqueId();
  const win = useWindow();
  let dialogRef: HTMLDivElement | undefined;
  let listRef: HTMLUListElement | undefined;

  createEffect(() => {
    if (!dialogRef) return;
    const teardown = installFocusTrap(dialogRef, {
      onEscape: () => closeNoteVersions(),
      fallbackRestore: () => {
        win.editor.focusEditor();
        return null;
      },
    });
    onCleanup(teardown);
  });

  function move(to: number) {
    const kept = noteVersionsStore.versions();
    const target = kept[to];
    if (!target || target.id === noteVersionsStore.selected()) return;
    void noteVersionsStore.select(target.id);
    requestAnimationFrame(() =>
      listRef?.querySelector<HTMLButtonElement>(`[data-version="${target.id}"]`)?.focus(),
    );
  }

  function onListKeyDown(event: KeyboardEvent) {
    const kept = noteVersionsStore.versions();
    if (kept.length === 0) return;
    const current = kept.findIndex((v) => v.id === noteVersionsStore.selected());
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        move(Math.min(kept.length - 1, current + 1));
        break;
      case "ArrowUp":
        event.preventDefault();
        move(Math.max(0, current - 1));
        break;
      case "Home":
        event.preventDefault();
        move(0);
        break;
      case "End":
        event.preventDefault();
        move(kept.length - 1);
        break;
    }
  }

  const selected = () =>
    noteVersionsStore.versions().find((v) => v.id === noteVersionsStore.selected()) ?? null;

  async function onRestore(id: number) {
    try {
      const restored = await noteVersionsStore.restore(id);
      showToast(`Restored ${nameOf(restored.note)}`);
    } catch (error) {
      showToast(readableError(error), "error");
    }
  }

  async function onCopy(id: number) {
    try {
      const copy = await noteVersionsStore.copy(id);
      showToast(`Copied to ${copy.name}`);
    } catch (error) {
      showToast(readableError(error), "error");
    }
  }

  return (
    <div class="note-versions-overlay" onClick={() => closeNoteVersions()}>
      <div
        ref={dialogRef}
        class="note-versions-modal"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
      >
        <div class="note-versions-header">
          <span id={titleId} class="note-versions-title">
            Earlier versions
          </span>
          <Show when={noteVersionsStore.path()}>
            {(path) => <span class="note-versions-note">{nameOf(path())}</span>}
          </Show>
          <span class="note-versions-header-controls">
            <Tooltip label="Close versions">
              <Button
                variant="ghost"
                icon="x"
                iconSize={16}
                onClick={closeNoteVersions}
                aria-label="Close versions"
              />
            </Tooltip>
          </span>
        </div>

        <Show
          when={noteVersionsStore.versions().length > 0}
          fallback={
            <Show
              when={!noteVersionsStore.loading()}
              fallback={
                <div class="note-versions-empty note-versions-loading">
                  <p class="note-versions-empty-line">Loading versions…</p>
                </div>
              }
            >
              <div class="note-versions-empty">
                <p class="note-versions-empty-line">No versions of this note yet.</p>
                <p class="note-versions-empty-hint">Writ keeps one each time the note is saved.</p>
              </div>
            </Show>
          }
        >
          <div class="note-versions-body">
            <div class="note-versions-side">
              <ul
                ref={listRef}
                class="note-versions-list"
                role="listbox"
                aria-label="Earlier versions"
                onKeyDown={onListKeyDown}
              >
                <For each={noteVersionsStore.versions()}>
                  {(version) => <Row version={version} />}
                </For>
              </ul>
              <p class="note-versions-kept">Restoring keeps the current text as another version.</p>
            </div>
            <div class="note-versions-reading">
              <pre class="note-versions-text">
                {noteVersionsStore.reading() ? "" : noteVersionsStore.text()}
              </pre>
              <Show when={selected()}>
                {(version) => (
                  <div class="note-versions-controls">
                    <Button
                      variant="primary"
                      data-action="restore-version"
                      onClick={() => void onRestore(version().id)}
                    >
                      Restore this version
                    </Button>
                    <Button
                      data-action="copy-version"
                      onClick={() => void onCopy(version().id)}
                    >
                      Copy this version
                    </Button>
                  </div>
                )}
              </Show>
            </div>
          </div>
        </Show>
      </div>
    </div>
  );
}

/**
 * What the note in front used to hold, newest first.
 *
 * Mounted only while it is open, and over the app rather than inside it: the
 * preview iframe is never unmounted (#127), and a panel that took the editor's
 * place would do exactly that.
 */
export default function NoteHistoryPanel() {
  return (
    <Show when={isOpen()}>
      <VersionsDialog />
    </Show>
  );
}
