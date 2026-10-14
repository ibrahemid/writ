import { Show, onMount } from "solid-js";
import { notesMigrationStore } from "../../stores/global/notes-migration";
import type { MoveArchiveOutcome } from "../../stores/global/notes-migration";
import { notesStore } from "../../stores/global/notes";
import Button from "../Button/Button";
import { showToast } from "../../stores/global/notifications";
import { logFailure } from "../../lib/log";
import { FILE_MANAGER_NAME, SHOW_IN_FILE_MANAGER } from "../../lib/platform";
import "./NotesMigrationReport.css";

/**
 * How many notes the pass put in the notes folder: the rows that already had a
 * file or got one, plus the piped input that became notes, plus the text that
 * could only be placed under `Recovered/`. All three are files the user will
 * find in the folder, so the panel counts them as one number.
 */
function placedCount(report: { migrated: number; piped: number; recovered: number }): number {
  return report.migrated + report.piped + report.recovered;
}

function fileWord(count: number): string {
  return count === 1 ? "file" : "files";
}

/** `1 file is now in ~/Writ.` / `4 files are now in ~/Writ.` */
function placedLine(count: number, where: string): string {
  return count === 1 ? `1 file is now in ${where}.` : `${count} files are now in ${where}.`;
}

function archivedLine(count: number): string {
  return count === 1
    ? "1 older file is waiting in an archive folder."
    : `${count} older files are waiting in an archive folder.`;
}

function failedLine(count: number): string {
  return count === 1 ? "1 file could not be checked." : `${count} files could not be checked.`;
}

/**
 * What the archive move did, including the notes that arrived under a
 * different name. Without that second sentence a user looking for
 * "Meeting.md" would not find it, because it landed as "Meeting 2.md".
 */
function archiveLine(outcome: MoveArchiveOutcome): string {
  const moved = `${outcome.moved} ${fileWord(outcome.moved)} moved into your folder.`;
  const renamed = outcome.collided.length;
  if (renamed === 0) return moved;
  if (renamed === 1) {
    return `${moved} One was renamed because your folder already had that name.`;
  }
  return `${moved} ${renamed} were renamed because your folder already had those names.`;
}

export default function NotesMigrationReport() {
  const report = () => notesMigrationStore.report();

  // Where the notes are now, not where the pass put them: the two differ once
  // the folder has been moved, and the one the user can go and look at is the
  // one worth naming.
  const where = () => notesStore.folder()?.display_path;

  onMount(() => {
    void notesMigrationStore.load();
    void notesStore.loadFolder().catch(() => {});
  });

  async function onShow() {
    try {
      await notesMigrationStore.showInFileManager();
    } catch {
      showToast(`Could not open ${FILE_MANAGER_NAME}`, "error");
    }
  }

  async function onShowRecovered() {
    try {
      await notesMigrationStore.showRecovered();
    } catch {
      showToast(`Could not open ${FILE_MANAGER_NAME}`, "error");
    }
  }

  async function onMoveArchived() {
    try {
      showToast(archiveLine(await notesMigrationStore.moveArchived()), "success");
    } catch {
      showToast("Could not move the archive", "error");
      logFailure("the archive could not be moved into the notes folder");
    }
  }

  return (
    <Show when={report()}>
      {(current) => (
        <div class="notes-report" role="status" aria-live="polite" data-notes-report>
          {/* A run can place nothing and still have something to say: the
              archive holds the closed notes, and a failure holds the rest. */}
          <Show when={placedCount(current()) > 0}>
            <div class="notes-report-line">
              <span class="notes-report-text">
                {placedLine(placedCount(current()), where() ?? current().notes_folder)}
              </span>
              <Button
                variant="ghost"
                class="notes-report-btn"
                data-action="notes-report-show"
                onClick={() => void onShow()}
              >
                {SHOW_IN_FILE_MANAGER}
              </Button>
            </div>
          </Show>

          <Show when={current().archived > 0}>
            <div class="notes-report-line">
              <span class="notes-report-text">{archivedLine(current().archived)}</span>
              <Button
                variant="ghost"
                class="notes-report-btn"
                data-action="notes-report-archive"
                onClick={() => void onMoveArchived()}
              >
                Move them into your folder
              </Button>
            </div>
          </Show>

          <Show when={current().failed > 0}>
            <div class="notes-report-line">
              <span class="notes-report-text">{failedLine(current().failed)}</span>
              <Button
                variant="ghost"
                class="notes-report-btn"
                data-action="notes-report-details"
                onClick={() => void onShowRecovered()}
              >
                {SHOW_IN_FILE_MANAGER}
              </Button>
            </div>
          </Show>

          <Button
            variant="ghost"
            icon="x"
            iconSize={16}
            class="notes-report-dismiss"
            data-action="notes-report-dismiss"
            aria-label="Dismiss the report"
            onClick={() => void notesMigrationStore.dismiss()}
          />
        </div>
      )}
    </Show>
  );
}
