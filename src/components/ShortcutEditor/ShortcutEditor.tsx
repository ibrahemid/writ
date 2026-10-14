import {
  createRoot,
  createSignal,
  createEffect,
  onCleanup,
  createMemo,
  untrack,
  For,
  Show,
} from "solid-js";
import { useAllCommands } from "../../commands/registry";
import { isCommandOffered } from "../../commands/app-commands";
import {
  effectiveBinding,
  rebuildKeyMap,
  setKeybindingOverrides,
} from "../../commands/keybindings";
import { ShortcutRecorder, findConflicts, findFixedConflicts } from "./recorder";
import { fixedMenuChords } from "../../commands/menu-commands";
import { UNUSABLE_GLOBAL_KEY } from "../../lib/chord";
import { detectPlatform } from "../../lib/platform";
import { configStore } from "../../stores/global/config";
import { hotkeyStore } from "../../stores/global/hotkey";
import Button from "../Button/Button";
import Icon from "../Icon/Icon";
import Kbd from "../Kbd/Kbd";
import { joinAnd } from "../../lib/join-and";
import { showToast } from "../../stores/global/notifications";
import {
  recordingId as listeningId,
  setRecordingId as setListeningId,
} from "../SettingsModal/recording";
import type { Command } from "../../types/commands";
import "./ShortcutEditor.css";

interface DraftEntry {
  binding: string;
}

/**
 * The row for the chord that shows and hides the window. Not a command: it is
 * registered with the OS rather than with Writ's own key map, which is why it
 * is the one row that can come back "taken".
 */
const GLOBAL_TOGGLE_ROW = "hotkey.toggle";
const GLOBAL_ROW_LABEL = "Show and hide Writ";
const GLOBAL_ROW_DESC = "Works while another app is in front";

// Singleton state — Writ is single-window. The list lives in Settings, and
// moving to another section or typing a search takes it off screen: the rows
// recorded and not saved stay here until Save, or until Settings closes.
const [drafts, setDrafts] = createSignal<Record<string, DraftEntry>>({});
const [globalDraft, setGlobalDraft] = createSignal("");
const [globalProblem, setGlobalProblem] = createSignal("");
let openSnapshot: Record<string, string> | null = null;

// The window's chord is the one row the OS can move, from Settings, General or
// from outside, whether the list is on screen or not. A status that lands is
// merged into that row and leaves the rest alone.
createRoot(() => {
  createEffect(() => {
    const held = hotkeyStore.chord();
    untrack(() => {
      if (!held) return;
      if (listeningId() === GLOBAL_TOGGLE_ROW) return;
      setGlobalDraft(held);
    });
  });
});

function listedCommands(): Command[] {
  return useAllCommands()
    .filter((c) => (c.scope === "app" || c.scope === "editor") && isCommandOffered(c))
    .slice()
    .sort((a, b) => a.label.localeCompare(b.label, undefined, { sensitivity: "base" }));
}

function draftBinding(cmd: Command): string {
  return drafts()[cmd.id]?.binding ?? configStore.config().keybindings[cmd.id] ?? cmd.keybinding ?? "";
}

function startDrafts(): void {
  openSnapshot = { ...configStore.config().keybindings };
  setDrafts({});
  setGlobalDraft(hotkeyStore.chord() || configStore.config().hotkey.toggle);
  setGlobalProblem("");
}

/** What a save would write: the rows that differ from the shipped chord,
 * and every saved chord of a command the list does not show, such as one
 * whose app is off, so switching an app off never costs its shortcuts. */
function pendingOverrides(commands: readonly Command[]): Record<string, string> {
  const listed = new Set(commands.map((cmd) => cmd.id));
  const next: Record<string, string> = {};
  for (const [id, binding] of Object.entries(configStore.config().keybindings)) {
    if (!listed.has(id)) next[id] = binding;
  }
  for (const cmd of commands) {
    const binding = draftBinding(cmd);
    if (binding && binding !== cmd.keybinding) {
      next[cmd.id] = binding;
    }
  }
  return next;
}

export function hasUnsavedShortcuts(): boolean {
  if (openSnapshot === null) return false;
  const saved = configStore.config().keybindings;
  const next = pendingOverrides(listedCommands());
  for (const id of new Set([...Object.keys(saved), ...Object.keys(next)])) {
    if (saved[id] !== next[id]) return true;
  }
  const toggle = globalDraft().trim();
  return toggle !== "" && toggle !== configStore.config().hotkey.toggle;
}

export function discardShortcutDrafts(): void {
  if (openSnapshot !== null) {
    setKeybindingOverrides(openSnapshot);
    rebuildKeyMap();
  }
  openSnapshot = null;
  setDrafts({});
  setGlobalDraft("");
  setGlobalProblem("");
  setListeningId(null);
}

export default function ShortcutEditor() {
  const recorder = new ShortcutRecorder();
  const [filter, setFilter] = createSignal("");
  const platform = detectPlatform();

  const commands = createMemo<Command[]>(() => listedCommands());

  const matchesFilter = (text: string): boolean => {
    const words = filter().trim().toLowerCase();
    return words === "" || text.toLowerCase().includes(words);
  };
  const listsCommand = (c: Command) => matchesFilter(`${c.label} ${c.description ?? ""}`);
  const showsGlobalRow = () => matchesFilter(`${GLOBAL_ROW_LABEL} ${GLOBAL_ROW_DESC}`);
  const appCommands = createMemo<Command[]>(() =>
    commands().filter((c) => c.scope === "app" && listsCommand(c)),
  );
  const editorCommands = createMemo<Command[]>(() =>
    commands().filter((c) => c.scope === "editor" && listsCommand(c)),
  );

  if (!untrack(hasUnsavedShortcuts)) untrack(startDrafts);

  function effectiveDraftMap(): Record<string, string> {
    const map: Record<string, string> = {};
    for (const cmd of commands()) {
      const binding = draftBinding(cmd);
      if (binding) map[cmd.id] = binding;
    }
    return map;
  }

  const conflicts = createMemo(() => findConflicts(effectiveDraftMap(), platform));
  // The macOS menu bar answers Quit, Hide, Copy and the rest before Writ sees
  // the key, so a command bound to one of them would never run.
  const fixedConflicts = createMemo(() =>
    findFixedConflicts(effectiveDraftMap(), fixedMenuChords(platform), platform),
  );
  const hasConflicts = () => conflicts().size > 0 || fixedConflicts().size > 0;

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
    if (!id) return;

    const handler = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      const outcome = recorder.handle(event);
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

  // A recording left running would keep Escape from closing Settings.
  onCleanup(() => {
    if (untrack(listeningId) !== null) stopRecording();
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
      const nextKeybindings = pendingOverrides(commands());
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
          setGlobalProblem(UNUSABLE_GLOBAL_KEY);
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
      showToast("Could not save the shortcuts", "error");
    }
  }

  // The one row the OS answers for. It carries the same recorder as the rest,
  // and one thing they cannot show: a chord another app is already holding.
  function renderGlobalRow() {
    const isListening = () => listeningId() === GLOBAL_TOGGLE_ROW;

    return (
      <div class="shortcut-row" data-shortcut="global-toggle">
        <div class="shortcut-row-info">
          <div class="shortcut-row-label">{GLOBAL_ROW_LABEL}</div>
          <div class="shortcut-row-desc">{GLOBAL_ROW_DESC}</div>
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
                when={globalDraft()}
                fallback={<span class="shortcut-row-empty">Not set</span>}
              >
                <Kbd binding={globalDraft()} />
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

  /** A conflicting row is named the way its command is named on screen. */
  function labelFor(commandId: string): string {
    return commands().find((c) => c.id === commandId)?.label ?? commandId;
  }

  function renderRow(cmd: Command) {
    const binding = () => draftBinding(cmd);
    const aliases = () => cmd.keybindingAliases ?? [];
    const isListening = () => listeningId() === cmd.id;
    const conflictWith = () => {
      const names = (conflicts().get(cmd.id) ?? []).map(labelFor);
      const fixed = fixedConflicts().get(cmd.id);
      return fixed === undefined ? names : [...names, fixed];
    };
    const isDefault = () => binding() === (cmd.keybinding ?? "");

    return (
      <div class="shortcut-row">
        <div class="shortcut-row-info">
          <div class="shortcut-row-label">
            <Show when={!isDefault()}>
              <span class="shortcut-row-changed" role="img" aria-label="Changed" />
            </Show>
            {cmd.label}
          </div>
          <Show when={cmd.description}>
            <div class="shortcut-row-desc">{cmd.description}</div>
          </Show>
          <Show when={aliases().length > 0}>
            <div class="shortcut-row-aliases">
              Also{" "}
              <For each={aliases()}>{(alias) => <Kbd binding={alias} muted />}</For>
            </div>
          </Show>
          <Show when={conflictWith().length > 0}>
            <div class="shortcut-row-conflict">
              Conflicts with {joinAnd(conflictWith())}
            </div>
          </Show>
        </div>
        <div class="shortcut-row-chip" aria-live="polite">
          <Show
            when={isListening()}
            fallback={
              <Show
                when={binding()}
                fallback={<span class="shortcut-row-empty">Not set</span>}
              >
                <Kbd binding={binding()} />
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
    <div class="shortcut-editor">
      <div class="shortcut-editor-header">
        <div class="shortcut-editor-filter">
          <Icon name="magnifying-glass" class="shortcut-editor-filter-icon" size={16} />
          <input
            type="text"
            class="shortcut-editor-filter-input"
            data-writ-focus-silent
            placeholder="Filter shortcuts"
            aria-label="Filter shortcuts"
            value={filter()}
            onInput={(e) => setFilter(e.currentTarget.value)}
          />
        </div>
        <div class="shortcut-editor-actions">
          <Show when={conflicts().size > 0}>
            <span class="shortcut-editor-blocked">Two commands share a shortcut.</span>
          </Show>
          <Show when={conflicts().size === 0 && fixedConflicts().size > 0}>
            <span class="shortcut-editor-blocked">The menu bar already uses a shortcut.</span>
          </Show>
          <Button data-action="reset-all-shortcuts" onClick={handleResetAll}>
            Reset all
          </Button>
          <Button
            data-action="save-shortcuts"
            variant="primary"
            disabled={hasConflicts()}
            onClick={handleSave}
          >
            Save
          </Button>
        </div>
      </div>

      <div class="shortcut-editor-body">
        <Show when={showsGlobalRow()}>
          <div class="shortcut-group-label">Window</div>
          {renderGlobalRow()}
        </Show>
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
        <Show
          when={!showsGlobalRow() && appCommands().length === 0 && editorCommands().length === 0}
        >
          <div class="shortcut-group-note">No shortcuts match.</div>
        </Show>
      </div>
    </div>
  );
}

export function bindingForCommand(commandId: string, fallback?: string): string | undefined {
  return effectiveBinding(commandId, fallback);
}
