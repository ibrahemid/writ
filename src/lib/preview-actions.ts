import { previewGetScripts, previewSetScripts } from "../services/tauri";

/**
 * Flip this file's preview-scripts opt-in and return the new value. Scripts are
 * off for every document until the file is opted in, and the choice is
 * remembered per file (decision RC-D1/TS-D1). The caller force-refreshes the
 * preview after this so the iframe reloads under the new document CSP.
 */
export async function togglePreviewScripts(bufferId: string): Promise<boolean> {
  const enabled = !(await previewGetScripts(bufferId));
  await previewSetScripts(bufferId, enabled);
  return enabled;
}
