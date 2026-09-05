import { wikilinkName, wikilinkTargetPath } from "../lib/wikilink";

/** What the index answered about a `[[…]]` target. */
export interface NoteLinkResolution {
  status: "resolved" | "ambiguous" | "missing";
  path: string | null;
  candidates: string[];
  /** The line the heading a `#` names sits on, when the index holds one. */
  heading_line?: number | null;
}

/**
 * Everything following a wikilink needs, injected so this file reaches no
 * store, no service and no component.
 */
export interface NoteLinkActions {
  resolve(fromPath: string, target: string): Promise<NoteLinkResolution>;
  /**
   * Opens a note by its path, at `line` when the target named a heading the
   * index knows the line of.
   */
  openPath(path: string, line?: number | null): void;
  /** Shows the notes a target could mean and opens the one that is picked. */
  showCandidates(
    name: string,
    candidates: string[],
    onPick: (path: string) => void,
  ): void;
  /** Offers to create the note a target names. */
  offerCreate(name: string, onCreate: () => void): void;
  /**
   * Creates the note a link target names, answering its path or null on
   * failure. The target still carries the folder it was written with and the
   * extension it was written with: the note is created in that folder, and the
   * one extension that comes off comes off in Rust, so taking either apart
   * here names a file the link does not resolve to.
   */
  create(target: string): Promise<string | null>;
}

/**
 * Follows a `[[…]]` written in the note at `fromPath`.
 *
 * One note opens it. Several ask which, because a target that names more than
 * one note is reported as ambiguous and never guessed (ADR-034). None offers
 * to create it, so writing a link and taking the offer leaves the link
 * pointing at a note.
 */
export async function followNoteLink(
  fromPath: string,
  target: string,
  actions: NoteLinkActions,
): Promise<void> {
  const written = target.trim();
  if (written === "") return;

  const resolution = await actions.resolve(fromPath, written);
  if (resolution.status === "resolved" && resolution.path) {
    // `[[Note#Section]]` asks for a place in the note, not just the note.
    actions.openPath(resolution.path, resolution.heading_line ?? null);
    return;
  }

  const name = wikilinkName(written);
  if (resolution.status === "ambiguous") {
    actions.showCandidates(name, resolution.candidates, (path) => actions.openPath(path));
    return;
  }
  if (name === "") return;
  const created = wikilinkTargetPath(written);
  actions.offerCreate(name, () => {
    void actions.create(created).then((path) => {
      if (path) actions.openPath(path);
    });
  });
}
