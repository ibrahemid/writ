import {
  createSignal,
  createEffect,
  onCleanup,
  createMemo,
  untrack,
  For,
  Show,
} from "solid-js";
import { useAllCommands } from "../../commands/registry";
import {
  effectiveBinding,
  rebuildKeyMap,
  setKeybindingOverrides,
} from "../../commands/keybindings";
import { ShortcutRecorder, findConflicts } from "./recorder";
import { keybindingSegments } from "../../lib/keybinding-format";
import { configStore } from "../../stores/global/config";
import { hotkeyStore } from "../../stores/global/hotkey";
import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
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

/**
 * The row for the chord that shows and hides the window. Not a command: it is
 * registered with the OS rather than with Writ's own key map, which is why it
 * is the one row that can come back "taken".
 */
const GLOBAL_TOGGLE_ROW = "hotkey.toggle";

export default function ShortcutEditor() {
  const win = useWindow();
  const recorder = new ShortcutRecorder();
  const [drafts, setDrafts] = createSignal<Record<string, DraftEntry>>({});
  const [globalDraft, setGlobalDraft] = createSignal("");
  const [globalProblem, setGlobalProblem] = createSignal("");
  const [listeningId, setListeningId] = createSignal<string | null>(null);
  let modalRef: HTMLDivElement | undefined;

  const commands = createMemo<Command[]>(() =>
    useAllCommands()
      .filter((c) => c.scope === "app" || c.scope === "editor")
      .slice()
      .sort((a, b) => a.label.localeCompare(b.label, undefined, { sensitivity: "base" })),
  );

  const appCommands = createMemo<Command[]>(() =>
    commands().filter((c) => c.scope === "app"),
  );
  const editorCommands = createMemo<Command[]>(() =>
    commands().filter((c) => c.scope === "editor"),
  );

  // Seeding happens on the way in and nowhere else. It used to run again on
  // every move of the command list, the config or the held chord, and each of
  // those threw away rows that had been recorded but not saved yet:
  // `hotkey:status` can land from outside the modal at any time.
  let wasOpen = false;

  createEffect(() => {
    if (!isOpen()) {
      wasOpen = false;
      return;
    }
    if (wasOpen) return;
    untrack(() => {
      const initial: Record<string, DraftEntry> = {};
      const overrides = configStore.config().keybindings;
      for (const cmd of commands()) {
        const binding = overrides[cmd.id] ?? cmd.keybinding ?? "";
        initial[cmd.id] = { binding };
      }
      setDrafts(initial);
      setGlobalDraft(hotkeyStore.chord() || configStore.config().hotkey.toggle);
      setGlobalProblem("");
      setListeningId(null);
      recorder.reset();
    });
    wasOpen = true;
  });

  // The window's chord is the one row the OS can move while the modal is up, so
  // a status that lands then is merged into that row and leaves the rest alone.
  createEffect(() => {
    const held = hotkeyStore.chord();
    untrack(() => {
      if (!isOpen() || !held) return;
      if (listeningId() === GLOBAL_TOGGLE_ROW) return;
      setGlobalDraft(held);
    });
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
        if (id === GLOBAL_TOGGLE_ROW) {
          setGlobalDraft(outcome.binding);
          setGlobalProblem("");
        } else setDraft(id, outcome.binding);
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
    setGlobalDraft(configStore.config().hotkey.toggle);
    setGlobalProblem("");
    const next: Record<string, DraftEntry> = {};
    for (const cmd of commands()) {
      next[cmd.id] = { binding: cmd.keybinding ?? "" };
    }
    setDrafts(next);
  }

  async function handleSave() {
    try {
      const nextToggle = globalDraft().trim();
      const toggleChanged = nextToggle !== "" && nextToggle !== configStore.config().hotkey.toggle;
      const nextKeybindings: Record<string, string> = {};
      for (const cmd of commands()) {
        const draft = drafts()[cmd.id];
        if (!draft) continue;
        if (draft.binding && draft.binding !== cmd.keybinding) {
          nextKeybindings[cmd.id] = draft.binding;
        }
      }
      // The OS holds this one, so it is asked before anything is written and
      // the config records what it gave back. The ask has a `try` of its own:
      // `set_global_hotkey` rejects a chord the hotkey parser cannot read, and
      // that refusal must cost the window's chord alone, never the rest of the
      // rows on screen.
      let toggle = configStore.config().hotkey.toggle;
      let toggleRefused = false;
      setGlobalProblem("");
      if (toggleChanged) {
        try {
          await hotkeyStore.rebind(nextToggle);
          toggle = hotkeyStore.chord() || toggle;
        } catch {
          toggleRefused = true;
          setGlobalProblem("Writ can't use this shortcut.");
        }
      }
      await configStore.save({
        ...configStore.config(),
        keybindings: nextKeybindings,
        hotkey: { ...configStore.config().hotkey, toggle },
      });
      setKeybindingOverrides(nextKeybindings);
      rebuildKeyMap();
      openSnapshot = { ...nextKeybindings };
      if (toggleRefused) showToast("Shortcuts saved. The window shortcut is unchanged.", "error");
      else showToast("Shortcuts saved", "success");
    } catch {
      showToast("Failed to save shortcuts", "error");
    }
  }

  // The one row the OS answers for. It carries the same recorder as the rest,
  // and one thing they cannot show: a chord another app is already holding.
  function renderGlobalRow() {
    const isListening = () => listeningId() === GLOBAL_TOGGLE_ROW;
    const segments = () => keybindingSegments(globalDraft());

    return (
      <div class="shortcut-row" data-shortcut="global-toggle">
        <div class="shortcut-row-info">
          <div class="shortcut-row-label">Show and hide Writ</div>
          <div class="shortcut-row-desc">Works while another app is in front</div>
          <Show when={hotkeyStore.isTaken()}>
            <div class="shortcut-row-conflict" data-state="taken">
              Another app is using this shortcut.
            </div>
          </Show>
          <Show when={globalProblem()}>
            <div class="shortcut-row-conflict" data-state="unusable">
              {globalProblem()}
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
                  <For each={segments()}>{(seg) => <span class="kbd-key">{seg}</span>}</For>
                </span>
              </Show>
            }
          >
            <span class="shortcut-row-listening">Press a key…</span>
          </Show>
        </div>
        <div class="shortcut-row-controls">
          <Button
            variant="ghost"
            data-action="record-global-shortcut"
            onClick={() =>
              isListening() ? stopRecording() : startRecording(GLOBAL_TOGGLE_ROW)
            }
          >
            {isListening() ? "Cancel" : "Record"}
          </Button>
          <Button
            variant="ghost"
            data-action="reset-global-shortcut"
            onClick={() => {
              setGlobalDraft(configStore.config().hotkey.toggle);
              setGlobalProblem("");
            }}
            disabled={globalDraft() === configStore.config().hotkey.toggle}
          >
            Reset
          </Button>
        </div>
      </div>
    );
  }

  function renderRow(cmd: Command) {
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
          <Button
            variant="ghost"
            data-action="record-shortcut"
            onClick={() => (isListening() ? stopRecording() : startRecording(cmd.id))}
          >
            {isListening() ? "Cancel" : "Record"}
          </Button>
          <Button
            variant="ghost"
            data-action="reset-shortcut"
            onClick={() => handleReset(cmd.id)}
            disabled={isDefault()}
          >
            Reset
          </Button>
        </div>
      </div>
    );
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
              <Button data-action="reset-all-shortcuts" onClick={handleResetAll}>
                Reset all
              </Button>
              <Button data-action="save-shortcuts" variant="primary" onClick={handleSave}>
                Save
              </Button>
              <Tooltip label="Close shortcut editor">
                <Button
                  variant="ghost"
                  icon="x"
                  iconSize={16}
                  onClick={closeShortcutEditor}
                  aria-label="Close shortcut editor"
                />
              </Tooltip>
            </div>
          </div>

          <div class="shortcut-editor-body">
            <div class="shortcut-group-label">Window</div>
            {renderGlobalRow()}
            <Show when={appCommands().length > 0}>
              <div class="shortcut-group-label">Commands</div>
              <For each={appCommands()}>{renderRow}</For>
            </Show>
            <Show when={editorCommands().length > 0}>
              <div class="shortcut-group-label">Editor</div>
              <div class="shortcut-group-note">
                Markdown formatting commands appear while a Markdown file is open.
              </div>
              <For each={editorCommands()}>{renderRow}</For>
            </Show>
          </div>
        </div>
      </div>
    </Show>
  );
}

export function bindingForCommand(commandId: string, fallback?: string): string | undefined {
  return effectiveBinding(commandId, fallback);
}
