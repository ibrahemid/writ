import { createSignal, createRoot } from "solid-js";
import * as api from "../../services/tauri";
import { onAutosaveSuccess } from "../../services/autosave";
import { bufferRegistry } from "./buffer-registry";
import { renameLinksStore } from "./rename-links";
import { configStore, withApp } from "./config";
import { windowRegistry } from "./window-registry";
import type { AppId, FileExtension } from "../../types/config";
import type { BufferDocument } from "../../types/buffer";
import { logFailure } from "../../lib/log";
import { APPS } from "../../lib/apps";
import { showToast } from "../../components/Notifications/Toast";

/** What the first launch is asking, in order. Null once it has nothing left
 * to ask. The Apps step is also opened later from Settings, Apps. */
export type FirstRunStep = "format" | "apps";

// Singleton state — Writ is single-window. What the first launch shows is
// read once and answered once, and the answer outlives the window.

/** The one line Writ shows under the cursor on a first launch. */
export function hintText(fileManager: string): string {
  return `Your files are saved automatically to a folder you can open in ${fileManager}.`;
}

/** The question Writ asks when it may not rename a note without asking. */
export function offerText(title: string): string {
  return `Rename this file to "${title}"?`;
}

const APP_IDS: readonly AppId[] = APPS.map((app) => app.id);

/** Exported for tests: each launch reads the state once, so a test that
 * needs a second launch needs a second store. */
export function createFirstRunStore() {
  const [step, setStep] = createSignal<FirstRunStep | null>(null);
  const [format, setFormat] = createSignal<FileExtension>("txt");
  const [showHint, setShowHint] = createSignal(false);
  const [fileManager, setFileManager] = createSignal("Finder");
  const [offer, setOffer] = createSignal<{ id: string; title: string } | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [apps, setApps] = createSignal<ReadonlySet<AppId>>(new Set());
  // The Apps step opened from Settings writes the switches and opens nothing;
  // the one a first launch reaches finishes that launch.
  const [revisiting, setRevisiting] = createSignal(false);
  let loaded = false;
  const asked = new Set<string>();

  // One read per launch. A launch that found a config file is not a first
  // one, so the line never appears for anyone upgrading.
  async function load(): Promise<void> {
    if (loaded) return;
    loaded = true;
    try {
      const state = await api.firstRunState();
      setFileManager(state.file_manager);
      setShowHint(state.first_run && !state.hint_dismissed);
      if (state.first_run) setStep("format");
    } catch {
      setShowHint(false);
    }
  }

  /** The format step's Continue: on to the Apps step, writing nothing yet, so
   * a person who quits there is asked both questions again. */
  function continueFormat(): void {
    if (step() !== "format") return;
    setStep("apps");
  }

  function isAppChosen(app: AppId): boolean {
    return apps().has(app);
  }

  function toggleApp(app: AppId): void {
    setApps((held) => {
      const next = new Set(held);
      if (next.has(app)) next.delete(app);
      else next.add(app);
      return next;
    });
  }

  /** Opens the Apps step from Settings, with each switch where it is now. */
  function showApps(): void {
    if (step() !== null) return;
    setApps(new Set(APP_IDS.filter((app) => configStore.isAppOn(app))));
    setRevisiting(true);
    setStep("apps");
  }

  /** Leaves the Apps step opened from Settings without writing. */
  function cancelApps(): void {
    if (!revisiting()) return;
    setRevisiting(false);
    setStep(null);
  }

  /** The Apps step's Continue. */
  async function continueApps(): Promise<void> {
    if (revisiting()) {
      await saveApps();
      return;
    }
    await continueSetup();
  }

  /** Writes the switches the revisited screen holds, in one config write, and
   * leaves. A write that fails keeps the screen up with the answer on it. */
  async function saveApps(): Promise<void> {
    if (busy()) return;
    setBusy(true);
    const chosen = apps();
    try {
      const next = APP_IDS.reduce(
        (config, app) => withApp(config, app, chosen.has(app)),
        configStore.config(),
      );
      await configStore.save(next);
    } catch {
      logFailure("the apps could not be saved");
      showToast("Could not save your apps", "error");
      return;
    } finally {
      setBusy(false);
    }
    setRevisiting(false);
    setStep(null);
  }

  /**
   * Takes both answers and leaves the screen.
   *
   * Nothing on disk has moved until this call: Rust writes the format and the
   * apps into the config in one write and answers with the note to open,
   * which is null on a launch that has tabs to restore. A call that fails
   * leaves the screen up with the answers still on it, because the config it
   * would have written is not there.
   *
   * One at a time. A held Enter repeats well inside one round trip, and two
   * calls that both reach the mint leave the folder with a
   * `writ-<yymmdd>-<hhmm>-2` nobody asked for. `busy()` is what the button
   * reads to say so.
   */
  async function continueSetup(): Promise<void> {
    if (busy()) return;
    setBusy(true);
    const extension = format();
    const chosen = APP_IDS.filter((app) => apps().has(app));
    let doc: BufferDocument | null;
    try {
      doc = await api.finishFirstRun(extension, chosen);
    } catch {
      logFailure("the first launch could not be finished");
      return;
    } finally {
      setBusy(false);
    }
    configStore.noteDefaultExtension(extension);
    configStore.noteAppsOn(chosen);
    // The screen leaves before the note arrives. While it is up it holds the
    // rest of the window inert, and an editor that mounts under that never
    // takes the focus it asks for, which would end the first launch on a note
    // the person has to click before they can type in it.
    setStep(null);
    if (doc) {
      bufferRegistry.adoptDocument(doc);
      windowRegistry.getActive()?.tabs.setActiveTabId(doc.id);
    }
  }

  // The line goes on the first keystroke and stays gone. The signal drops
  // first: a line still on screen while Writ writes the config reads as a
  // keystroke that did nothing.
  function dismissHint(): void {
    if (!showHint()) return;
    setShowHint(false);
    configStore.noteFirstRunHintDismissed();
    api.dismissFirstRunHint().catch(() => logFailure("the first-run line could not be recorded"));
  }

  /**
   * Asks what a note's first line may do to the note's file name.
   *
   * Rust holds the two facts that decide it — whether the tab has been closed,
   * and whether anything outside Writ has touched the path — and reads the
   * line off the file the save just wrote. The answer is the rename, a
   * question, a note with no first line yet, or nothing at all.
   */
  async function offerRetitle(id: string): Promise<void> {
    if (asked.has(id)) return;
    const outcome = await api.autoRetitleNote(id);
    // "Not yet" is the empty note, which every later save may still answer.
    // "Nothing to do" is every other note in the workspace, and is final, so
    // the ordinary save of an ordinary note stops asking after the first one.
    if (outcome.kind === "not_yet") return;
    asked.add(id);
    if (outcome.kind === "skipped") return;
    if (outcome.kind === "renamed") {
      await bufferRegistry.refreshBuffer(id);
      return;
    }
    setOffer({ id, title: outcome.title });
  }

  /**
   * Takes the offer: the ordinary rename, and the notes that link to the note
   * follow it.
   *
   * The unasked rename skips the links because the note was minted this
   * launch and its tab has never been closed, so a `[[…]]` naming its date can
   * only have been typed in the same session. The offer is the other case: it
   * is reached because the tab was closed or something outside Writ touched
   * the file, which is when a link to it can already exist.
   */
  async function acceptOffer(): Promise<void> {
    const current = offer();
    if (current === null) return;
    setOffer(null);
    await renameLinksStore.renameWithLinks(current.id, current.title, true);
  }

  /** Leaves the note under the date it was named for. */
  function dismissOffer(): void {
    setOffer(null);
  }

  return {
    step,
    format,
    setFormat,
    continueFormat,
    isAppChosen,
    toggleApp,
    showApps,
    continueApps,
    cancelApps,
    revisiting,
    busy,
    showHint,
    fileManager,
    offer,
    load,
    dismissHint,
    offerRetitle,
    acceptOffer,
    dismissOffer,
  };
}

export const firstRunStore = createRoot(createFirstRunStore);

/**
 * Asks about the note's name each time a save lands.
 *
 * A save is the moment the first line has stopped moving, which is when the
 * name it would mint is worth acting on; a keystroke is not. Rust answers
 * "nothing to do" for every note this does not apply to, so there is no
 * second rule here about which notes to ask about.
 */
export function watchSavesForRetitle(): () => void {
  return onAutosaveSuccess((id) => {
    firstRunStore.offerRetitle(id).catch(() => logFailure("the note could not be renamed"));
  });
}
