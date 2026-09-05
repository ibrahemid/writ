export type ExternalChange = "modified" | "removed" | "moved";

export type ExternalEditAction =
  | "ignore"
  | "follow"
  | "mark-removed"
  | "reload"
  | "prompt";

export interface ExternalEditInputs {
  change: ExternalChange;
  known: boolean;
  hasUnsaved: boolean;
  /** Whether the tab is already marked as having lost its file. */
  removedOnDisk?: boolean;
}

// Decides how to respond to an external change to a buffer's backing file.
//
// An unknown file is ignored. A file that moved changes no bytes, so the tab
// follows it to its new path and nothing is read, reloaded or asked: putting a
// move through the dirty gate would throw away unsaved text over a rename. A
// file that was deleted marks the tab, which keeps the text and stops the next
// save recreating the file (spec W4). A modification reloads the editor from
// disk when there is nothing to lose, and asks first when there are unsaved
// edits the reload would discard.
//
// A tab that already lost its file reads a second removal as nothing new: it
// says what the first one said, and acting on it again would cancel a queue
// the mark has since put text back into. A file at the note's own path again
// takes the same fork as any other modification, because it is the same
// question: a tab holding text no file has is asked which version the file
// ends up with, and a tab holding nothing of its own reads the file back
// quietly (ADR-033 §15).
export function planExternalEdit(inputs: ExternalEditInputs): ExternalEditAction {
  if (!inputs.known) return "ignore";
  if (inputs.change === "moved") return "follow";
  if (inputs.change === "removed") {
    return inputs.removedOnDisk ? "ignore" : "mark-removed";
  }
  return inputs.hasUnsaved ? "prompt" : "reload";
}

export interface ExternalEditBuffer {
  id: string;
  title: string;
}

export interface ExternalEditDeps {
  findBuffer: (idOrFilename: string) => ExternalEditBuffer | undefined;
  hasUnsaved: (id: string) => boolean;
  isRemovedOnDisk: (id: string) => boolean;
  reload: (id: string) => void;
  // Raises the bar that asks what to do about a file that changed under text
  // no file holds. Nothing is read, replaced or written until it is answered.
  markChanged: (id: string) => void;
  // Repoints the tab at the file's new path: its name, the path it saves to,
  // and the folder it is watched in. The text is untouched.
  followMove: (id: string, newPath: string) => void;
  // Marks the tab as having no file on disk. The store takes the text it is
  // the last copy of and cancels the queue, in that order, so this must not
  // be paired with a `cancelAutosave` of its own.
  markRemoved: (id: string) => void;
}

// What the backend says about a file that changed outside Writ. `path` names
// the file, `diskHash` is what it holds now, and `newPath` is where it went
// for a change that is a move. Only `bufferId` and `change` are read here; the
// rest are what the reload and the move handling are built on.
export interface ExternalEditPayload {
  bufferId: string;
  change: ExternalChange;
  path?: string;
  newPath?: string | null;
  diskHash?: string | null;
}

// Reads a `buffer:external` event off the wire, or rejects it.
//
// The guard the whole feature passes through, which is why it is here with a
// test rather than inline at the subscription. Rust named the fields
// `buffer_id` and `change` for a while; every payload arrived with `bufferId`
// undefined, this check dropped it, and the feature was silently inert for as
// long as that lasted. A rename on either side has to fail a test, not a user.
export function readExternalEditPayload(payload: {
  bufferId?: string;
  change?: string;
  path?: string;
  newPath?: string | null;
  diskHash?: string | null;
}): ExternalEditPayload | null {
  if (!payload.bufferId) return null;
  if (
    payload.change !== "modified" &&
    payload.change !== "removed" &&
    payload.change !== "moved"
  ) {
    return null;
  }
  return {
    bufferId: payload.bufferId,
    change: payload.change,
    path: payload.path,
    newPath: payload.newPath,
    diskHash: payload.diskHash,
  };
}

// Resolves and executes the response to a `buffer:external` event.
//
// Deliberately never reloads the global buffer registry: a blanket registry
// reload re-creates the always-mounted preview pane's iframe and hard-freezes
// the macOS webview (PR#127). Only the editor's own content is reset, via
// `reload`, which reads the file through Rust rather than from any copy Writ
// is holding.
export async function handleExternalEdit(
  payload: ExternalEditPayload,
  deps: ExternalEditDeps,
): Promise<void> {
  const buffer = deps.findBuffer(payload.bufferId);
  const action = planExternalEdit({
    change: payload.change,
    known: buffer !== undefined,
    hasUnsaved: buffer ? deps.hasUnsaved(buffer.id) : false,
    removedOnDisk: buffer ? deps.isRemovedOnDisk(buffer.id) : false,
  });

  if (!buffer || action === "ignore") return;

  switch (action) {
    case "follow":
      // A move that names nowhere is not a move anything can follow. It cannot
      // happen from the backend, and silence beats repointing a tab at "".
      if (payload.newPath) deps.followMove(buffer.id, payload.newPath);
      return;
    case "mark-removed":
      deps.markRemoved(buffer.id);
      return;
    case "reload":
      deps.reload(buffer.id);
      return;
    case "prompt":
      // The store cancels the queue as it raises the bar, and reads what the
      // queue was carrying first: for a background tab that is the only copy
      // of its text, and cancelling first is what threw it away.
      deps.markChanged(buffer.id);
      return;
  }
}
