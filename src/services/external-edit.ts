export type ExternalChange = "modified" | "deleted";

export type ExternalEditAction = "ignore" | "toast" | "reload" | "prompt";

export interface ExternalEditInputs {
  change: ExternalChange;
  known: boolean;
  hasUnsaved: boolean;
}

// Decides how to respond to an external change to a buffer's backing file
// (audit blocker #53.4). An unknown file is ignored; a deletion only
// notifies (the in-memory buffer keeps its content and recreates the file
// on the next save); a modification reloads the editor from disk when there
// is nothing to lose, and prompts first when the user has unsaved edits that
// the reload would discard.
export function planExternalEdit(inputs: ExternalEditInputs): ExternalEditAction {
  if (!inputs.known) return "ignore";
  if (inputs.change === "deleted") return "toast";
  return inputs.hasUnsaved ? "prompt" : "reload";
}

export interface ExternalEditBuffer {
  id: string;
  title: string;
}

export interface ExternalEditDeps {
  findBuffer: (idOrFilename: string) => ExternalEditBuffer | undefined;
  hasUnsaved: (id: string) => boolean;
  reload: (id: string) => void;
  cancelAutosave: (id: string) => void;
  toast: (message: string, level: "warning") => void;
  confirmReload: (title: string) => Promise<boolean>;
}

// Resolves and executes the response to a `buffer:external` event.
//
// Deliberately never reloads the global buffer registry: a blanket registry
// reload re-creates the always-mounted preview pane's iframe and hard-freezes
// the macOS webview (PR#127). Only the editor's own content is reset, via
// `reload`.
export async function handleExternalEdit(
  payload: { bufferId: string; change: ExternalChange },
  deps: ExternalEditDeps,
): Promise<void> {
  const buffer = deps.findBuffer(payload.bufferId);
  const action = planExternalEdit({
    change: payload.change,
    known: buffer !== undefined,
    hasUnsaved: buffer ? deps.hasUnsaved(buffer.id) : false,
  });

  if (!buffer || action === "ignore") return;

  switch (action) {
    case "toast":
      deps.toast(`File "${buffer.title}" deleted externally`, "warning");
      return;
    case "reload":
      deps.reload(buffer.id);
      return;
    case "prompt": {
      const reload = await deps.confirmReload(buffer.title);
      if (reload) {
        deps.cancelAutosave(buffer.id);
        deps.reload(buffer.id);
      }
      return;
    }
  }
}
