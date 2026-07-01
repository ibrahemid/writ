import { createSignal, createEffect, onCleanup, createMemo, For, Show } from "solid-js";
import { useAllCommands } from "../../commands/registry";
import {
  effectiveBinding,
  rebuildKeyMap,
  setKeybindingOverrides,
} from "../../commands/keybindings";
import { ShortcutRecorder, findConflicts } from "./recorder";
import { keybindingSegments } from "../../lib/keybinding-format";
import { configStore } from "../../stores/global/config";
import { useWindow } from "../WindowProvider/WindowProvider";
import { installFocusTrap } from "../../lib/focus-trap";
import { showToast } from "../Notifications/Toast";
import type { Command } from "../../types/commands";
import "./ShortcutEditor.css";

// Singleton state — Writ is single-window
const [isOpen, setIsOpen] = createSignal(false);
let openSnapshot: Record<string, string> | null = null;

export function openShortcutEditor() {
  openSnapshot = { ...configStore.config().keybindings };
  setIsOpen(true);
}

export function closeShortcutEditor() {
  if (openSnapshot !== null) {
    setKeybindingOverrides(openSnapshot);
    rebuildKeyMap();
  }
  openSnapshot = null;
  setIsOpen(false);
}

interface DraftEntry {
  binding: string;
}

export default function ShortcutEditor() {
  const win = useWindow();
  const recorder = new ShortcutRecorder();
  const [drafts, setDrafts] = createSignal<Record<string, DraftEntry>>({});
  const [listeningId, setListeningId] = createSignal<string | null>(null);
  let modalRef: HTMLDivElement | undefined;

  const commands = createMemo<Command[]>(() =>
    useAllCommands()
      .filter((c) => c.scope === "app")
      .slice()
      .sort((a, b) => a.label.localeCompare(b.label, undefined, { sensitivity: "base" })),
  );

  createEffect(() => {
    if (!isOpen()) return;
    const initial: Record<string, DraftEntry> = {};
    const overrides = configStore.config().keybindings;
    for (const cmd of commands()) {
      const binding = overrides[cmd.id] ?? cmd.keybinding ?? "";
      initial[cmd.id] = { binding };
    }
    setDrafts(initial);
    setListeningId(null);
    recorder.reset();
  });

  function effectiveDraftMap(): Record<string, string> {
    const map: Record<string, string> = {};
    for (const [id, entry] of Object.entries(drafts())) {
      if (entry.binding) map[id] = entry.binding;
    }
    return map;
  }

  const conflicts = createMemo(() => findConflicts(effectiveDraftMap()));

  function defaultBindingFor(commandId: string): string {
    const cmd = commands().find((c) => c.id === commandId);
    return cmd?.keybinding ?? "";
  }

  function setDraft(id: string, binding: string) {
    setDrafts((prev) => ({ ...prev, [id]: { binding } }));
  }

  function startRecording(id: string) {
    recorder.reset();
    setListeningId(id);
  }

  function stopRecording() {
    recorder.reset();
    setListeningId(null);
  }

  createEffect(() => {
    const id = listeningId();
    if (!id || !isOpen()) return;

    const handler = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      const outcome = recorder.handle({
        key: event.key,
        metaKey: event.metaKey,
        ctrlKey: event.ctrlKey,
        shiftKey: event.shiftKey,
        altKey: event.altKey,
      });
      if (outcome.kind === "captured") {
        setDraft(id, outcome.binding);
        stopRecording();
      } else if (outcome.kind === "cancelled") {
        stopRecording();
      }
    };

    document.addEventListener("keydown", handler, { capture: true });
    onCleanup(() => document.removeEventListener("keydown", handler, { capture: true }));
  });

  createEffect(() => {
    if (!isOpen() || !modalRef) return;
    const teardown = installFocusTrap(modalRef, {
      isActive: () => listeningId() === null,
      onEscape: () => closeShortcutEditor(),
      fallbackRestore: () => {
        win.editor.focusEditor();
        return null;
      },
    });
    onCleanup(teardown);
  });

  function handleReset(id: string) {
    setDraft(id, defaultBindingFor(id));
  }

  function handleResetAll() {
    const next: Record<string, DraftEntry> = {};
    for (const cmd of commands()) {
      next[cmd.id] = { binding: cmd.keybinding ?? "" };
    }
    setDrafts(next);
  }

  async function handleSave() {
    try {
      const nextKeybindings: Record<string, string> = {};
      for (const cmd of commands()) {
        const draft = drafts()[cmd.id];
        if (!draft) continue;
        if (draft.binding && draft.binding !== cmd.keybinding) {
          nextKeybindings[cmd.id] = draft.binding;
        }
      }
      await configStore.save({
        ...configStore.config(),
        keybindings: nextKeybindings,
      });
      setKeybindingOverrides(nextKeybindings);
      rebuildKeyMap();
      openSnapshot = { ...nextKeybindings };
      showToast("Shortcuts saved", "success");
    } catch {
      showToast("Failed to save shortcuts", "error");
    }
  }

  return (
    <Show when={isOpen()}>
      <div class="shortcut-editor-overlay" onClick={() => closeShortcutEditor()}>
        <div
          ref={modalRef}
          class="shortcut-editor"
          onClick={(e) => e.stopPropagation()}
          role="dialog"
          aria-modal="true"
          aria-label="Customize shortcuts"
        >
          <div class="shortcut-editor-header">
            <div class="shortcut-editor-title">Customize shortcuts</div>
            <div class="shortcut-editor-actions">
              <button type="button" class="shortcut-editor-btn" onClick={handleResetAll}>
                Reset all
              </button>
              <button
                type="button"
                class="shortcut-editor-btn shortcut-editor-btn-primary"
                onClick={handleSave}
              >
                Save
              </button>
              <button
                type="button"
                class="shortcut-editor-close"
                onClick={closeShortcutEditor}
                aria-label="Close shortcut editor"
                title="Close"
              >
                <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
                  <path
                    d="M2 2L10 10M10 2L2 10"
                    stroke="currentColor"
                    stroke-width="1.5"
                    stroke-linecap="round"
                  />
                </svg>
              </button>
            </div>
          </div>

          <div class="shortcut-editor-body">
            <For each={commands()}>
              {(cmd) => {
                const binding = () => drafts()[cmd.id]?.binding ?? "";
                const aliases = () => cmd.keybindingAliases ?? [];
                const isListening = () => listeningId() === cmd.id;
                const conflictWith = () => conflicts().get(cmd.id) ?? [];
                const segments = () => keybindingSegments(binding());
                const isDefault = () => binding() === (cmd.keybinding ?? "");

                return (
                  <div class="shortcut-row">
                    <div class="shortcut-row-info">
                      <div class="shortcut-row-label">{cmd.label}</div>
                      <Show when={cmd.description}>
                        <div class="shortcut-row-desc">{cmd.description}</div>
                      </Show>
                      <Show when={aliases().length > 0}>
                        <div class="shortcut-row-aliases">
                          ({aliases().length} {aliases().length === 1 ? "alias" : "aliases"}, read-only)
                        </div>
                      </Show>
                      <Show when={conflictWith().length > 0}>
                        <div class="shortcut-row-conflict">
                          Conflicts with {conflictWith().join(", ")}
                        </div>
                      </Show>
                    </div>
                    <div class="shortcut-row-chip" aria-live="polite">
                      <Show
                        when={isListening()}
                        fallback={
                          <Show
                            when={segments().length > 0}
                            fallback={<span class="shortcut-row-empty">unset</span>}
                          >
                            <span class="kbd-chord">
                              <For each={segments()}>
                                {(seg) => <span class="kbd-key">{seg}</span>}
                              </For>
                            </span>
                          </Show>
                        }
                      >
                        <span class="shortcut-row-listening">Press a key…</span>
                      </Show>
                    </div>
                    <div class="shortcut-row-controls">
                      <button
                        type="button"
                        class="shortcut-row-btn"
                        onClick={() => (isListening() ? stopRecording() : startRecording(cmd.id))}
                      >
                        {isListening() ? "Cancel" : "Record"}
                      </button>
                      <button
                        type="button"
                        class="shortcut-row-btn"
                        onClick={() => handleReset(cmd.id)}
                        disabled={isDefault()}
                      >
                        Reset
                      </button>
                    </div>
                  </div>
                );
              }}
            </For>
          </div>
        </div>
      </div>
    </Show>
  );
}

export function bindingForCommand(commandId: string, fallback?: string): string | undefined {
  return effectiveBinding(commandId, fallback);
}
