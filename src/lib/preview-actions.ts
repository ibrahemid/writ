import { previewGetScripts, previewSetScripts } from "../services/tauri";

/**
 * Flip this file's preview-scripts opt-in and return the new value. Scripts are
 * off for every document until the file is opted in, and the choice is
 * remembered per file (decision RC-D1/TS-D1). The caller force-refreshes the
 * preview after this so the iframe reloads under the new document CSP.
 *
 * `shown` is the opt-in the user is looking at, flipped as shown; the stored
 * one is read only when nothing is shown. An opt-in can lapse after it was
 * shown, when a render finds the file changed (ADR-043), and a click on
 * `Scripts on` must turn the scripts off rather than opt the changed file in.
 */
export async function togglePreviewScripts(bufferId: string, shown?: boolean): Promise<boolean> {
  const enabled = !(shown ?? (await previewGetScripts(bufferId)));
  await previewSetScripts(bufferId, enabled);
  return enabled;
}
