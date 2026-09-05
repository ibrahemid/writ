import { wikilinkName } from "../lib/wikilink";

/** What the index answered about a `[[…]]` target. */
export interface NoteLinkResolution {
  status: "resolved" | "ambiguous" | "missing";
  path: string | null;
  candidates: string[];
}

/**
 * Everything following a wikilink needs, injected so this file reaches no
 * store, no service and no component.
 */
export interface NoteLinkActions {
  resolve(fromPath: string, target: string): Promise<NoteLinkResolution>;
  /** Opens a note by its path. */
  openPath(path: string): void;
  /** Shows the notes a target could mean and opens the one that is picked. */
  showCandidates(
    name: string,
    candidates: string[],
    onPick: (path: string) => void,
  ): void;
  /** Offers to create the note a target names. */
  offerCreate(name: string, onCreate: () => void): void;
  /** Creates a note called `name`, answering its path or null on failure. */
  create(name: string): Promise<string | null>;
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
    actions.openPath(resolution.path);
    return;
  }

  const name = wikilinkName(written);
  if (resolution.status === "ambiguous") {
    actions.showCandidates(name, resolution.candidates, (path) => actions.openPath(path));
    return;
  }
  if (name === "") return;
  actions.offerCreate(name, () => {
    void actions.create(name).then((path) => {
      if (path) actions.openPath(path);
    });
  });
}
