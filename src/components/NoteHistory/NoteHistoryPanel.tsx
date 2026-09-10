import { createEffect, createSignal, For, onCleanup, Show, createUniqueId } from "solid-js";

import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
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
    <li>
      <button
        type="button"
        class="note-versions-row"
        data-version={props.version.id}
        aria-current={isSelected() ? "true" : undefined}
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
  let dialogRef: HTMLDivElement | undefined;

  createEffect(() => {
    if (!dialogRef) return;
    const teardown = installFocusTrap(dialogRef, { onEscape: () => closeNoteVersions() });
    onCleanup(teardown);
  });

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
            <div class="note-versions-empty">
              <p class="note-versions-empty-line">No versions of this note yet.</p>
              <p class="note-versions-empty-hint">Writ keeps one each time the note is saved.</p>
            </div>
          }
        >
          <div class="note-versions-body">
            <ul class="note-versions-list">
              <For each={noteVersionsStore.versions()}>
                {(version) => <Row version={version} />}
              </For>
            </ul>
            <div class="note-versions-reading">
              <pre class="note-versions-text">{noteVersionsStore.text()}</pre>
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
