import type { NoteDiskAnswer } from "./tauri";
import type { ExternalEditPayload } from "./external-edit";

export interface SweptNote {
  id: string;
}

// How many notes are asked after at once.
export const SWEEP_CONCURRENCY = 4;

export interface NotesSweepDeps {
  // The tab registry. Only its open tabs are asked after: the registry also
  // holds hundreds of closed-tab history rows, and each question reads and
  // hashes a whole file.
  tabs: { activeTabs: () => SweptNote[] };
  // What the note's file holds now, read through Rust.
  diskStateOf: (id: string) => Promise<NoteDiskAnswer>;
  // What Writ last recorded that file holding.
  lastKnownDiskHash: (id: string) => string | undefined;
  // The same route a named change takes.
  onChanged: (payload: ExternalEditPayload) => void | Promise<void>;
  // A note whose question failed; the others are still asked.
  onFailure: (id: string) => void;
}

// Re-checks every open tab after the watcher reports that the notes folder
// changed faster than it could be listed.
//
// A sweep says the folder moved without saying which files did, so the tabs
// have to ask. One question per open tab, answered by the file itself, and a
// note whose file still holds what Writ recorded produces nothing. Up to
// SWEEP_CONCURRENCY questions are in flight at once, and one that fails is
// reported and skipped rather than stopping the tabs after it.
//
// Two notes are passed over rather than reported:
//
// A note with no recorded hash is one Writ has not read this launch — a tab
// restored at launch and never brought to the front. There is nothing to
// compare against, and claiming a change would put a discard-your-work prompt
// over a document nobody has typed into. It reads its file when it mounts, so
// it is not asked at all.
//
// A note whose file cannot be described is either gone or not downloaded yet.
// Telling those apart is the write guard's job and it already fails closed;
// guessing here would either lose a note to a sync client that had not
// finished, or hide a deletion behind a wrong message.
//
// It never reloads the buffer registry. A blanket registry reload re-creates
// the always-mounted preview iframe and hard-freezes the macOS webview
// (PR #127); the tabs are reached one at a time through the ordinary
// external-change route.
export async function recheckOpenNotes(deps: NotesSweepDeps): Promise<void> {
  const queue = deps.tabs
    .activeTabs()
    .map((note) => ({ id: note.id, known: deps.lastKnownDiskHash(note.id) }))
    .filter((note): note is { id: string; known: string } => note.known !== undefined);
  let next = 0;

  async function askInTurn(): Promise<void> {
    while (next < queue.length) {
      const note = queue[next++]!;
      try {
        const answer = await deps.diskStateOf(note.id);
        if (answer.state !== "described" || answer.disk.hash === note.known) continue;
        await deps.onChanged({
          bufferId: note.id,
          change: "modified",
          diskHash: answer.disk.hash,
        });
      } catch {
        deps.onFailure(note.id);
      }
    }
  }

  await Promise.all(
    Array.from({ length: Math.min(SWEEP_CONCURRENCY, queue.length) }, () => askInTurn()),
  );
}
