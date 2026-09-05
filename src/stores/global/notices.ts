import { openThirdPartyNotices } from "../../services/tauri";
import { bufferRegistry } from "./buffer-registry";
import type { BufferDocument } from "../../types/buffer";

export const THIRD_PARTY_NOTICES_TITLE = "Third-party licences";

export interface NoticesBuffer {
  doc: BufferDocument;
  /** The caller must reload the editor view: this buffer was already open. */
  reused: boolean;
}

/**
 * Open the bundled notices as a read-only buffer, reusing the one a previous
 * open left.
 *
 * The backend regenerates the file at a fixed path under the data directory
 * on every call and never mints it into the notes folder or the search index
 * (ADR-028 §1); identity is the file's path, the same as reopening anything
 * else, so a second call resolves to the same buffer rather than a duplicate
 * tab.
 */
export async function openThirdPartyNoticesBuffer(): Promise<NoticesBuffer> {
  const result = await openThirdPartyNotices();
  // Writ writes this file itself, so it is always here: the placeholder mode
  // belongs to notes a sync provider has not downloaded.
  const doc = result.doc;
  if (!doc) throw new Error("the notices document opened no note");
  const reused = bufferRegistry.activeTabs().some((buffer) => buffer.id === doc.id);
  bufferRegistry.registerOpenResult(result);
  return { doc, reused };
}
