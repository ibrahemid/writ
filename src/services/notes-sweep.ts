import type { NoteDiskAnswer } from "./tauri";
import type { ExternalEditPayload } from "./external-edit";

export interface SweptNote {
  id: string;
}

export interface NotesSweepDeps {
  // Every note with a tab open on it.
  openNotes: () => SweptNote[];
  // What the note's file holds now, read through Rust.
  diskStateOf: (id: string) => Promise<NoteDiskAnswer>;
  // What Writ last recorded that file holding.
  lastKnownDiskHash: (id: string) => string | undefined;
  // The same route a named change takes.
  onChanged: (payload: ExternalEditPayload) => void | Promise<void>;
}

// Re-checks every open note after the watcher reports that the notes folder
// changed faster than it could be listed.
//
// A sweep says the folder moved without saying which files did, so the tabs
// have to ask. One question per open note, answered by the file itself, and a
// note whose file still holds what Writ recorded produces nothing.
//
// Two notes are passed over rather than reported:
//
// A note with no recorded hash is one Writ has not read this launch — a tab
// restored at launch and never brought to the front. There is nothing to
// compare against, and claiming a change would put a discard-your-work prompt
// over a document nobody has typed into. It reads its file when it mounts.
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
  for (const note of deps.openNotes()) {
    const known = deps.lastKnownDiskHash(note.id);
    const answer = await deps.diskStateOf(note.id);
    if (answer.state !== "described") continue;
    if (known === undefined || answer.disk.hash === known) continue;
    await deps.onChanged({
      bufferId: note.id,
      change: "modified",
      diskHash: answer.disk.hash,
    });
  }
}
