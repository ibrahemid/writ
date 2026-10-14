import { createEffect, createSignal, onCleanup, onMount, Show, untrack } from "solid-js";
import { configStore } from "../../../stores/global/config";
import { hotkeyStore } from "../../../stores/global/hotkey";
import { notesStore } from "../../../stores/global/notes";
import type { NotesFallbackReason } from "../../../stores/global/notes";
import { showToast } from "../../../stores/global/notifications";
import { UNUSABLE_GLOBAL_KEY } from "../../../lib/chord";
import { joinAnd } from "../../../lib/join-and";
import { logFailure } from "../../../lib/log";
import { FILE_MANAGER_NAME, SHOW_IN_FILE_MANAGER } from "../../../lib/platform";
import type { FileExtension } from "../../../types/config";
import Button from "../../Button/Button";
import Kbd from "../../Kbd/Kbd";
import { ShortcutRecorder } from "../../ShortcutEditor/recorder";
import { NotesSyncNote } from "../NotesSyncNote";
import { PathText, SectionLabel, patchConfig } from "../controls";
import { recordingId, setRecordingId } from "../recording";
import { FormatSample } from "../previews";
import { SettingsRow } from "../SettingsRow";

/**
 * Says where the files went when the folder in the settings could not be used.
 *
 * The refused path is not named: it is the one the user typed or picked, it is
 * on the same row above this line, and repeating it says nothing the reader
 * does not already have.
 */
function fallbackLine(displayPath: string, reason: NotesFallbackReason): string {
  const why =
    reason === "holds_writ_data" ? "holds Writ's own data" : "could not be used";
  return `The folder in your settings ${why}, so files are in ${displayPath}.`;
}

/** Names the files a move would have written over, at most three of them. */
function collisionLine(names: string[]): string {
  const shown = names.slice(0, 3).map((name) => `“${name}”`).join(", ");
  const rest = names.length - 3;
  const list = rest > 0 ? `${shown} and ${rest} more` : shown;
  return `That folder already has ${list}. Nothing moved.`;
}

function FolderRow() {
  const folder = () => notesStore.folder();

  onMount(() => {
    void notesStore.loadFolder().catch(() => {});
  });

  async function onShow() {
    try {
      await notesStore.showInFileManager();
    } catch {
      showToast(`Could not open ${FILE_MANAGER_NAME}`, "error");
    }
  }

  async function onCopy() {
    try {
      await notesStore.copyPath();
      showToast("Path copied", "success");
    } catch {
      showToast("Could not copy the path", "error");
    }
  }

  async function onMove() {
    try {
      const outcome = await notesStore.move();
      if (!outcome) return;
      if (outcome.collided.length > 0) {
        showToast(collisionLine(outcome.collided), "error");
        return;
      }
      const where = folder()?.display_path ?? outcome.new_root;
      if (outcome.left_behind.length > 0) {
        showToast(
          `Your files are now in ${where}. A copy of ${joinAnd(outcome.left_behind)} is still in the old folder.`,
          "info",
          8000,
        );
        return;
      }
      showToast(`Your files are now in ${where}.`, "success");
    } catch {
      showToast("Could not move the folder", "error");
      logFailure("the folder could not be moved");
    }
  }

  return (
    <SettingsRow id="notes.folder">
      <span class="settings-notes">
        <span class="settings-notes-controls">
          <PathText
            class="settings-notes-path"
            data-notes-path
            full={folder()?.path ?? ""}
            text={folder()?.display_path ?? "…"}
          />
          <Button data-action="notes-show" onClick={() => void onShow()}>
            {SHOW_IN_FILE_MANAGER}
          </Button>
          <Button data-action="notes-copy" onClick={() => void onCopy()}>
            Copy path
          </Button>
          <Button data-action="notes-move" onClick={() => void onMove()}>
            Move…
          </Button>
        </span>
        <Show when={folder()?.fallback}>
          {(fallback) => (
            <span class="settings-notes-note" data-notes-fallback>
              {fallbackLine(folder()?.display_path ?? "", fallback().reason)}
            </span>
          )}
        </Show>
        <NotesSyncNote provider={folder()?.sync_provider ?? null} />
      </span>
    </SettingsRow>
  );
}

const GLOBAL_CHORD_ROW = "general.hotkey.toggle";

function GlobalChordRow() {
  const recorder = new ShortcutRecorder();
  const [problem, setProblem] = createSignal("");
  const listening = () => recordingId() === GLOBAL_CHORD_ROW;
  const chord = () => hotkeyStore.chord() || configStore.config().hotkey.toggle;

  function stopRecording() {
    recorder.reset();
    setRecordingId(null);
  }

  function startRecording() {
    recorder.reset();
    setProblem("");
    setRecordingId(GLOBAL_CHORD_ROW);
  }

  async function apply(next: string) {
    try {
      await hotkeyStore.rebind(next);
    } catch {
      setProblem(UNUSABLE_GLOBAL_KEY);
      return;
    }
    const toggle = hotkeyStore.chord() || next;
    await patchConfig((prev) => ({ ...prev, hotkey: { ...prev.hotkey, toggle } }));
  }

  createEffect(() => {
    if (!listening()) return;
    const handler = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      const outcome = recorder.handle(event);
      if (outcome.kind === "captured") {
        stopRecording();
        void apply(outcome.binding);
      } else if (outcome.kind === "cancelled") {
        stopRecording();
      }
    };
    document.addEventListener("keydown", handler, { capture: true });
    onCleanup(() => document.removeEventListener("keydown", handler, { capture: true }));
  });

  // A recording left running would keep Escape from closing Settings.
  onCleanup(() => {
    if (untrack(listening)) stopRecording();
  });

  const caution = () =>
    problem() || (hotkeyStore.isTaken() ? "Another app is using this shortcut." : undefined);

  return (
    <SettingsRow id="hotkey.toggle" caution={caution()}>
      <span class="settings-chord" aria-live="polite">
        <Show
          when={listening()}
          fallback={
            <Show when={chord()} fallback={<span class="settings-chord-note">Not set</span>}>
              <Kbd binding={chord()} />
            </Show>
          }
        >
          <span class="settings-chord-note">Press a key…</span>
        </Show>
        <Button
          data-action="record-global-chord"
          onClick={() => (listening() ? stopRecording() : startRecording())}
        >
          {listening() ? "Cancel" : "Record"}
        </Button>
      </span>
    </SettingsRow>
  );
}

export default function GeneralSection() {
  function onDefaultExtensionChange(raw: string) {
    const extension = raw as FileExtension;
    void patchConfig((prev) => ({ ...prev, files: { ...prev.files, default_extension: extension } }));
  }

  return (
    <div data-section="general">
      <SectionLabel section="general" />
      <FolderRow />
      <SettingsRow
        id="files.default_extension"
        labelFor="setting-default-extension"
        detail={<FormatSample extension={configStore.config().files.default_extension} />}
      >
        <select
          id="setting-default-extension"
          class="settings-select"
          data-setting="default_extension"
          value={configStore.config().files.default_extension}
          onChange={(e) => onDefaultExtensionChange(e.currentTarget.value)}
        >
          <option value="txt">Plain text (.txt)</option>
          <option value="md">Markdown (.md)</option>
        </select>
      </SettingsRow>
      <GlobalChordRow />
    </div>
  );
}
