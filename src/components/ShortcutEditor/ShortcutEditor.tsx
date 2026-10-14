import {
  createSignal,
  createEffect,
  createUniqueId,
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
import {
  ShortcutRecorder,
  findConflicts,
  findFixedConflicts,
  type RecorderOutcome,
} from "./recorder";
import { fixedMenuChords } from "../../commands/menu-commands";
import { detectPlatform, type Platform } from "../../lib/platform";
import { sameChord, worksFromAnyApp } from "../../lib/chord";
import { formatKeybinding } from "../../lib/keybinding-format";
import { configStore } from "../../stores/global/config";
import {
  captureHotkeyStore,
  hotkeyStore,
  SHIPPED_CAPTURE_CHORD,
  SHIPPED_WINDOW_CHORD,
} from "../../stores/global/hotkey";
import Button from "../Button/Button";
import Icon from "../Icon/Icon";
import Kbd from "../Kbd/Kbd";
import Tooltip from "../Tooltip/Tooltip";
import { useWindow } from "../WindowProvider/WindowProvider";
import { installFocusTrap } from "../../lib/focus-trap";
import { joinAnd } from "../../lib/join-and";
import { requestChoice } from "../../stores/global/dialogs";
import { showToast } from "../../stores/global/notifications";
import type { Command } from "../../types/commands";
import "./ShortcutEditor.css";

// Singleton state — Writ is single-window
const [isOpen, setIsOpen] = createSignal(false);
let openSnapshot: Record<string, string> | null = null;
let recordWindowChordOnOpen = false;

interface OpenShortcutEditorOptions {
  /** Open listening on the window's chord, for a chord another app holds. */
  recordWindowChord?: boolean;
}

export function openShortcutEditor(options: OpenShortcutEditorOptions = {}) {
  openSnapshot = { ...configStore.config().keybindings };
  recordWindowChordOnOpen = options.recordWindowChord === true;
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

/** Why the window's row refused a chord: the rule it breaks, another app
 * holding it, or a key Writ cannot register. */
interface WindowChordProblem {
  state: "rule" | "refused" | "unusable";
  text: string;
}

interface DraftEntry {
  binding: string;
}

/** One row whose chord the OS holds for every app. */
interface AnyAppRow {
  id: string;
  label: string;
  desc?: string;
  dataShortcut: string;
  recordAction: string;
  resetAction: string;
  shipped: string;
  draft: () => string;
  setDraft: (chord: string) => void;
  problem: () => WindowChordProblem | null;
  setProblem: (problem: WindowChordProblem | null) => void;
  isTaken: () => boolean;
  isUnsupported: () => boolean;
}

/**
 * The row for the chord that shows and hides the window. Not a command: it is
 * registered with the OS rather than with Writ's own key map, which is why it
 * is the one row that can come back "taken".
 */
const GLOBAL_TOGGLE_ROW = "hotkey.toggle";
const GLOBAL_ROW_LABEL = "Show and hide Writ";
/** The heading over both rows the OS holds for every app. */
const ANY_APP_GROUP = "From any app";

/** The row for the chord that opens the quick capture note (ADR-055), held
 * by the OS the same way. */
const CAPTURE_ROW = "hotkey.capture";
const CAPTURE_ROW_LABEL = "Quick capture";
const CAPTURE_ROW_DESC = "Adds a line to today's file";

/** What the window's chord cannot do on a Wayland desktop. */
export const WAYLAND_LINE = "Wayland doesn't pass this shortcut to Writ from other apps.";

/** The rule a chord for the window has to keep, as the row states it. */
export function windowChordRule(platform: Platform): string {
  return platform === "mac"
    ? "A shortcut that works from any app needs Command, Control or Option. F13 to F20 work on their own."
    : "A shortcut that works from any app needs Ctrl or Alt. F13 to F24 work on their own.";
}

export default function ShortcutEditor() {
  const win = useWindow();
  const titleId = createUniqueId();
  const recorder = new ShortcutRecorder();
  const [drafts, setDrafts] = createSignal<Record<string, DraftEntry>>({});
  const [globalDraft, setGlobalDraft] = createSignal("");
  const [globalProblem, setGlobalProblem] = createSignal<WindowChordProblem | null>(null);
  const [captureDraft, setCaptureDraft] = createSignal("");
  const [captureProblem, setCaptureProblem] = createSignal<WindowChordProblem | null>(null);
  const [listeningId, setListeningId] = createSignal<string | null>(null);
  const [filter, setFilter] = createSignal("");
  const platform = detectPlatform();
  let modalRef: HTMLDivElement | undefined;

  const commands = createMemo<Command[]>(() =>
    useAllCommands()
      .filter((c) => (c.scope === "app" || c.scope === "editor") && isCommandOffered(c))
      .slice()
      .sort((a, b) => a.label.localeCompare(b.label, undefined, { sensitivity: "base" })),
  );

  const matchesFilter = (text: string): boolean => {
    const words = filter().trim().toLowerCase();
    return words === "" || text.toLowerCase().includes(words);
  };
  const listsCommand = (c: Command) => matchesFilter(`${c.label} ${c.description ?? ""}`);
  const showsGlobalRow = () => matchesFilter(`${GLOBAL_ROW_LABEL} ${ANY_APP_GROUP}`);
  const showsCaptureRow = () =>
    matchesFilter(`${CAPTURE_ROW_LABEL} ${CAPTURE_ROW_DESC} ${ANY_APP_GROUP}`);
  const savedCapture = () => configStore.config().hotkey.capture ?? SHIPPED_CAPTURE_CHORD;
  const heldCapture = () => captureHotkeyStore.chord() || savedCapture();
  const appCommands = createMemo<Command[]>(() =>
    commands().filter((c) => c.scope === "app" && listsCommand(c)),
  );
  const editorCommands = createMemo<Command[]>(() =>
    commands().filter((c) => c.scope === "editor" && listsCommand(c)),
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
      setGlobalProblem(null);
      setCaptureDraft(heldCapture());
      setCaptureProblem(null);
      setListeningId(recordWindowChordOnOpen ? GLOBAL_TOGGLE_ROW : null);
      recordWindowChordOnOpen = false;
      setFilter("");
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

  createEffect(() => {
    const held = captureHotkeyStore.chord();
    untrack(() => {
      if (!isOpen() || !held) return;
      if (listeningId() === CAPTURE_ROW) return;
      setCaptureDraft(held);
    });
  });

  function effectiveDraftMap(): Record<string, string> {
    const map: Record<string, string> = {};
    for (const [id, entry] of Object.entries(drafts())) {
      if (entry.binding) map[id] = entry.binding;
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
    if (!id || !isOpen()) return;

    const handler = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      const outcome = recorder.handle(event);
      if (outcome.kind === "cancelled") {
        stopRecording();
      } else if (id === GLOBAL_TOGGLE_ROW) {
        recordWindowChord(outcome);
      } else if (id === CAPTURE_ROW) {
        recordCaptureChord(outcome);
      } else if (outcome.kind === "captured") {
        setDraft(id, outcome.binding);
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
      onEscape: () => void requestClose(),
      fallbackRestore: () => {
        win.editor.focusEditor();
        return null;
      },
    });
    onCleanup(teardown);
  });

  // A chord the OS would hold for every app has to leave the other apps their
  // keys: refused, the row says the rule and keeps listening.
  function recordWindowChord(outcome: RecorderOutcome) {
    if (outcome.kind === "bare") {
      setGlobalProblem({ state: "rule", text: windowChordRule(platform) });
      return;
    }
    if (outcome.kind !== "captured") return;
    if (!worksFromAnyApp(outcome.binding)) {
      setGlobalProblem({ state: "rule", text: windowChordRule(platform) });
      recorder.reset();
      return;
    }
    if (captureDraft() !== "" && sameChord(outcome.binding, captureDraft())) {
      setGlobalProblem({ state: "rule", text: `${CAPTURE_ROW_LABEL} uses this shortcut.` });
      recorder.reset();
      return;
    }
    setGlobalDraft(outcome.binding);
    setGlobalProblem(null);
    stopRecording();
  }

  // The capture row keeps the same rule, and cannot take the window's chord:
  // the OS holds one chord for one of them.
  function recordCaptureChord(outcome: RecorderOutcome) {
    if (outcome.kind === "bare") {
      setCaptureProblem({ state: "rule", text: windowChordRule(platform) });
      return;
    }
    if (outcome.kind !== "captured") return;
    if (!worksFromAnyApp(outcome.binding)) {
      setCaptureProblem({ state: "rule", text: windowChordRule(platform) });
      recorder.reset();
      return;
    }
    if (sameChord(outcome.binding, globalDraft())) {
      setCaptureProblem({ state: "rule", text: `${GLOBAL_ROW_LABEL} uses this shortcut.` });
      recorder.reset();
      return;
    }
    setCaptureDraft(outcome.binding);
    setCaptureProblem(null);
    stopRecording();
  }

  function handleReset(id: string) {
    setDraft(id, defaultBindingFor(id));
  }

  function handleResetAll() {
    setGlobalDraft(SHIPPED_WINDOW_CHORD);
    setGlobalProblem(null);
    setCaptureDraft(SHIPPED_CAPTURE_CHORD);
    setCaptureProblem(null);
    const next: Record<string, DraftEntry> = {};
    for (const cmd of commands()) {
      next[cmd.id] = { binding: cmd.keybinding ?? "" };
    }
    setDrafts(next);
  }

  /** What a save would write: the rows that differ from the shipped chord,
   * and every saved chord of a command the editor does not list, such as one
   * whose app is off, so switching an app off never costs its shortcuts. */
  function pendingOverrides(): Record<string, string> {
    const listed = new Set(commands().map((cmd) => cmd.id));
    const next: Record<string, string> = {};
    for (const [id, binding] of Object.entries(configStore.config().keybindings)) {
      if (!listed.has(id)) next[id] = binding;
    }
    for (const cmd of commands()) {
      const draft = drafts()[cmd.id];
      if (!draft) continue;
      if (draft.binding && draft.binding !== cmd.keybinding) {
        next[cmd.id] = draft.binding;
      }
    }
    return next;
  }

  function isDirty(): boolean {
    const saved = configStore.config().keybindings;
    const next = pendingOverrides();
    for (const id of new Set([...Object.keys(saved), ...Object.keys(next)])) {
      if (saved[id] !== next[id]) return true;
    }
    // Against the chord held rather than the one saved: a saved chord the
    // rule refuses fell back at startup, and the row opens on the fallback.
    const toggle = globalDraft().trim();
    const held = hotkeyStore.chord() || configStore.config().hotkey.toggle;
    if (toggle !== "" && toggle !== held) return true;
    return captureDraft().trim() !== heldCapture();
  }

  async function requestClose() {
    if (!isDirty()) {
      closeShortcutEditor();
      return;
    }
    const outcome = await requestChoice({
      title: "Discard your changes?",
      message: "Writ will put the shortcuts back the way they were.",
      confirmLabel: "Discard",
      defaultAction: "cancel",
    });
    if (outcome === "confirm") closeShortcutEditor();
  }

  async function handleSave() {
    try {
      const nextToggle = globalDraft().trim();
      const toggleChanged = nextToggle !== "" && nextToggle !== configStore.config().hotkey.toggle;
      const nextKeybindings = pendingOverrides();
      // The OS holds this one, so it is asked before anything is written and
      // the config records what it gave back. The ask has a `try` of its own:
      // `set_global_hotkey` rejects a chord the hotkey parser cannot read, and
      // that refusal must cost the window's chord alone, never the rest of the
      // rows on screen.
      let toggle = configStore.config().hotkey.toggle;
      let toggleRefused = false;
      setGlobalProblem(null);
      if (toggleChanged) {
        try {
          const given = await hotkeyStore.rebind(nextToggle);
          toggle = hotkeyStore.chord() || toggle;
          if (!given) {
            toggleRefused = true;
            setGlobalProblem({
              state: "refused",
              text: `Another app is using ${formatKeybinding(nextToggle)}.`,
            });
          }
        } catch {
          toggleRefused = true;
          setGlobalProblem({ state: "unusable", text: "Writ can't use this shortcut." });
        }
      }
      // The capture chord the same way, on its own: one the OS turns down
      // leaves the window's chord and the rest of the rows as saved.
      const nextCapture = captureDraft().trim();
      let capture: { capture?: string } = {};
      let captureRefused = false;
      setCaptureProblem(null);
      if (nextCapture !== savedCapture()) {
        try {
          const given = await captureHotkeyStore.rebind(nextCapture);
          if (given) capture = { capture: nextCapture };
          else {
            captureRefused = true;
            setCaptureProblem({
              state: "refused",
              text: `Another app is using ${formatKeybinding(nextCapture)}.`,
            });
          }
        } catch {
          captureRefused = true;
          setCaptureProblem({ state: "unusable", text: "Writ can't use this shortcut." });
        }
      }
      await configStore.save({
        ...configStore.config(),
        keybindings: nextKeybindings,
        hotkey: { ...configStore.config().hotkey, toggle, ...capture },
      });
      setKeybindingOverrides(nextKeybindings);
      rebuildKeyMap();
      openSnapshot = { ...nextKeybindings };
      if (toggleRefused) showToast("Shortcuts saved. The window shortcut is unchanged.", "error");
      else if (captureRefused)
        showToast("Shortcuts saved. The quick capture shortcut is unchanged.", "error");
      else showToast("Shortcuts saved", "success");
    } catch {
      showToast("Could not save the shortcuts", "error");
    }
  }

  // The rows the OS answers for. They carry the same recorder as the rest, and
  // one thing the others cannot show: a chord another app is already holding.
  function renderAnyAppRow(row: AnyAppRow) {
    const isListening = () => listeningId() === row.id;

    return (
      <div class="shortcut-row" data-shortcut={row.dataShortcut}>
        <div class="shortcut-row-info">
          <div class="shortcut-row-label">{row.label}</div>
          <Show when={row.desc}>
            <div class="shortcut-row-desc">{row.desc}</div>
          </Show>
          <Show when={row.isTaken()}>
            <div class="shortcut-row-conflict" data-state="taken">
              Another app is using this shortcut.
            </div>
          </Show>
          <Show when={row.isUnsupported()}>
            <div class="shortcut-row-conflict" data-state="unsupported">
              {WAYLAND_LINE}
            </div>
          </Show>
          <Show when={row.problem()}>
            {(problem) => (
              <div class="shortcut-row-conflict" data-state={problem().state}>
                {problem().text}
              </div>
            )}
          </Show>
        </div>
        <div class="shortcut-row-chip" aria-live="polite">
          <Show
            when={isListening()}
            fallback={
              <Show
                when={row.draft()}
                fallback={<span class="shortcut-row-empty">Not set</span>}
              >
                <Kbd binding={row.draft()} />
              </Show>
            }
          >
            <span class="shortcut-row-listening">Press a key…</span>
          </Show>
        </div>
        <div class="shortcut-row-controls">
          <Button
            variant="ghost"
            data-action={row.recordAction}
            onClick={() => (isListening() ? stopRecording() : startRecording(row.id))}
          >
            {isListening() ? "Cancel" : "Record"}
          </Button>
          <Button
            variant="ghost"
            data-action={row.resetAction}
            onClick={() => {
              row.setDraft(row.shipped);
              row.setProblem(null);
            }}
            disabled={row.draft() === row.shipped}
          >
            Reset
          </Button>
        </div>
      </div>
    );
  }

  const windowRow: AnyAppRow = {
    id: GLOBAL_TOGGLE_ROW,
    label: GLOBAL_ROW_LABEL,
    dataShortcut: "global-toggle",
    recordAction: "record-global-shortcut",
    resetAction: "reset-global-shortcut",
    shipped: SHIPPED_WINDOW_CHORD,
    draft: globalDraft,
    setDraft: setGlobalDraft,
    problem: globalProblem,
    setProblem: setGlobalProblem,
    isTaken: hotkeyStore.isTaken,
    isUnsupported: hotkeyStore.isUnsupported,
  };

  const captureRow: AnyAppRow = {
    id: CAPTURE_ROW,
    label: CAPTURE_ROW_LABEL,
    desc: CAPTURE_ROW_DESC,
    dataShortcut: "global-capture",
    recordAction: "record-capture-shortcut",
    resetAction: "reset-capture-shortcut",
    shipped: SHIPPED_CAPTURE_CHORD,
    draft: captureDraft,
    setDraft: setCaptureDraft,
    problem: captureProblem,
    setProblem: setCaptureProblem,
    isTaken: captureHotkeyStore.isTaken,
    isUnsupported: captureHotkeyStore.isUnsupported,
  };

  /** A conflicting row is named the way its command is named on screen. */
  function labelFor(commandId: string): string {
    return commands().find((c) => c.id === commandId)?.label ?? commandId;
  }

  function renderRow(cmd: Command) {
    const binding = () => drafts()[cmd.id]?.binding ?? "";
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
              ({aliases().length} {aliases().length === 1 ? "alias" : "aliases"}, read-only)
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
    <Show when={isOpen()}>
      <div class="shortcut-editor-overlay" onClick={() => void requestClose()}>
        <div
          ref={modalRef}
          class="shortcut-editor"
          onClick={(e) => e.stopPropagation()}
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
        >
          <div class="shortcut-editor-header">
            <div id={titleId} class="shortcut-editor-title">
              Customize shortcuts
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
              <Tooltip label="Close shortcut editor" chord="Escape">
                <Button
                  variant="ghost"
                  icon="x"
                  iconSize={16}
                  onClick={() => void requestClose()}
                  aria-label="Close shortcut editor"
                />
              </Tooltip>
            </div>
          </div>

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

          <div class="shortcut-editor-body">
            <Show when={showsGlobalRow() || showsCaptureRow()}>
              <div class="shortcut-group-label">{ANY_APP_GROUP}</div>
              <Show when={showsGlobalRow()}>{renderAnyAppRow(windowRow)}</Show>
              <Show when={showsCaptureRow()}>{renderAnyAppRow(captureRow)}</Show>
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
              when={
                !showsGlobalRow() &&
                !showsCaptureRow() &&
                appCommands().length === 0 &&
                editorCommands().length === 0
              }
            >
              <div class="shortcut-group-note">No shortcuts match.</div>
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
