import type { Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { ClipboardPasteResult } from "../types/image-import";

/**
 * Pasting images into a Markdown file.
 *
 * The host reads the clipboard and saves the images beside the file, then
 * answers with the references to insert, so no image bytes cross into the
 * webview. Injected, so this module reaches no service and no store.
 */
export interface ImagePasteDeps {
  /** The tab the editor holds now. */
  bufferId(): string;
  /** Whether the tab is a Markdown file that can be edited. */
  acceptsImages(): boolean;
  pasteImages(bufferId: string): Promise<ClipboardPasteResult>;
  readText(): Promise<string>;
  /** Shows each message to the person. */
  notify(notices: readonly string[]): void;
  /** Records a host failure for the log. */
  logFailure(message: string): void;
}

export const TAB_CHANGED_NOTICE = "The tab changed before the paste finished";
export const CLIPBOARD_CHANGED_NOTICE = "The clipboard changed before the image was pasted";
export const PASTE_FAILED_NOTICE = "Could not paste the image";

// A line copied file names arrive as when a file manager also puts the names
// on the clipboard as text.
const FILE_NAME_LINE = /^[^\\/:*?"<>|\t]*[^\s.]\.[A-Za-z0-9]{1,8}$/;
const FILE_NAME_LIMIT = 4096;

/**
 * Whether a paste event's clipboard may hold images, which the host is then
 * asked about. Plain text that is not a list of file names stays with the
 * editor's own paste.
 */
export function clipboardMayHoldImages(data: DataTransfer | null): boolean {
  if (!data) return false;
  const types = Array.from(data.types ?? []);
  if (types.includes("Files")) return true;
  if (types.some((type) => type.startsWith("image/"))) return true;
  const text = data.getData("text/plain");
  if (text.length === 0) return true;
  if (text.length > FILE_NAME_LIMIT) return false;
  const lines = text.split(/\r\n|\r|\n/).filter((line) => line.length > 0);
  return lines.length > 0 && lines.every((line) => FILE_NAME_LINE.test(line.trim()));
}

/** What the paste event saw on the clipboard, or null for a menu paste. */
export interface ClipboardSnapshot {
  text: string;
  sawImage: boolean;
}

function insertAtSelection(view: EditorView, text: string): void {
  view.dispatch({
    ...view.state.replaceSelection(text),
    scrollIntoView: true,
    userEvent: "input.paste",
  });
}

/**
 * Asks the host to paste the clipboard's images into the tab, and pastes the
 * clipboard's text when it holds no images.
 *
 * `snapshot` is what a paste event saw; a menu paste has none and reads the
 * text when the host finds no images. The insert is one transaction, so one
 * undo removes it.
 */
export async function pasteImagesOrText(
  view: EditorView,
  deps: ImagePasteDeps,
  snapshot: ClipboardSnapshot | null,
): Promise<void> {
  const bufferId = deps.bufferId();
  let result: ClipboardPasteResult;
  try {
    result = await deps.pasteImages(bufferId);
  } catch {
    deps.logFailure("the clipboard could not be pasted into the file");
    if (snapshot && (snapshot.sawImage || !snapshot.text)) {
      deps.notify([PASTE_FAILED_NOTICE]);
      return;
    }
    result = { kind: "text" };
  }
  if (result.kind === "refused") {
    deps.notify(result.notices);
    return;
  }
  const text =
    result.kind === "inserted"
      ? result.markdown
      : snapshot
        ? snapshot.text
        : await deps.readText();
  if (deps.bufferId() !== bufferId) {
    if (text) deps.notify([TAB_CHANGED_NOTICE]);
    return;
  }
  if (result.kind === "inserted") {
    if (text) insertAtSelection(view, text);
    if (result.notices.length > 0) deps.notify(result.notices);
    view.focus();
    return;
  }
  if (text) {
    insertAtSelection(view, text);
    view.focus();
    return;
  }
  if (snapshot?.sawImage) deps.notify([CLIPBOARD_CHANGED_NOTICE]);
}

/**
 * Takes a paste whose clipboard may hold images away from the editor's own
 * paste in a Markdown tab, and leaves every other paste to it.
 */
export function imagePaste(deps: ImagePasteDeps): Extension {
  return EditorView.domEventHandlers({
    paste(event, view) {
      if (view.state.readOnly || !deps.acceptsImages()) return false;
      const data = event.clipboardData;
      if (!clipboardMayHoldImages(data)) return false;
      event.preventDefault();
      const types = Array.from(data?.types ?? []);
      const snapshot: ClipboardSnapshot = {
        text: data?.getData("text/plain") ?? "",
        sawImage: types.includes("Files") || types.some((type) => type.startsWith("image/")),
      };
      void pasteImagesOrText(view, deps, snapshot);
      return true;
    },
  });
}
