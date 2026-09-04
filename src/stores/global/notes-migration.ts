import { createSignal, createRoot } from "solid-js";
import * as api from "../../services/tauri";
import { joinPath } from "../../lib/path";
import type { MoveArchiveOutcome, NotesMigrationReport } from "../../services/tauri";

// Singleton state — Writ is single-window. The report is read once per launch
// and shown until the user dismisses it, after which no launch shows it again.

export type { MoveArchiveOutcome, NotesMigrationReport };

function createNotesMigrationStore() {
  const [report, setReport] = createSignal<NotesMigrationReport | null>(null);
  const [loaded, setLoaded] = createSignal(false);

  // One read per launch. The backend answers null once the report has been
  // dismissed and for a run that placed nothing, so there is no second rule
  // here about when to draw the panel.
  async function load(): Promise<void> {
    if (loaded()) return;
    setLoaded(true);
    try {
      setReport(await api.getNotesMigrationReport());
    } catch {
      setReport(null);
    }
  }

  // The panel goes as soon as it is dismissed, whether or not the row records
  // it: a panel that stays on screen while Writ writes to the database reads
  // as a button that did nothing.
  async function dismiss(): Promise<void> {
    setReport(null);
    await api.dismissNotesMigrationReport();
  }

  async function showInFileManager(): Promise<void> {
    return api.showNotesFolderInFinder();
  }

  // The folder the pass wrote text into when it could not place it with
  // confidence. Its name is fixed by the migration
  // (`writ_storage::notes_migration::RECOVERED_FOLDER`). The notes folder is
  // asked for rather than read off the report, because a folder the user has
  // since moved took this one with it.
  async function showRecovered(): Promise<void> {
    const folder = await api.getNotesFolder();
    return api.showNotesFileInFileManager(joinPath(folder.path, "Recovered"));
  }

  // The backend re-counts the stored report, so a note that would not move
  // stays counted there. The panel follows the same arithmetic rather than
  // clearing the offer outright, or a note left behind would lose its way back.
  async function moveArchived(): Promise<MoveArchiveOutcome> {
    const outcome = await api.moveArchivedNotes();
    const current = report();
    if (current) {
      setReport({
        ...current,
        archived: Math.max(0, current.archived - outcome.moved),
        migrated: current.migrated + outcome.moved,
      });
    }
    return outcome;
  }

  return { report, load, dismiss, showInFileManager, showRecovered, moveArchived };
}

export const notesMigrationStore = createRoot(createNotesMigrationStore);
