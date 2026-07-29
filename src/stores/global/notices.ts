import { readThirdPartyNotices, saveBufferContentUnindexed } from "../../services/tauri";
import { cancelAutosave } from "../../services/autosave";
import { bufferRegistry } from "./buffer-registry";
import type { BufferDocument } from "../../types/buffer";

export const THIRD_PARTY_NOTICES_TITLE = "Third-party licences";

// The first line scripts/gen-third-party-notices.py writes. A buffer is only
// treated as a previous open's if its content starts with this: the title alone
// would let a buffer the user named the same thing be overwritten.
const NOTICES_HEADING = "# Third-party notices";

async function findOpenNoticesBuffer(): Promise<BufferDocument | null> {
  const candidates = bufferRegistry
    .activeTabs()
    .filter((buffer) => buffer.title === THIRD_PARTY_NOTICES_TITLE);
  for (const candidate of candidates) {
    const content = await bufferRegistry.readContent(candidate.id).catch(() => null);
    if (content?.startsWith(NOTICES_HEADING)) return candidate;
  }
  return null;
}

export interface NoticesBuffer {
  doc: BufferDocument;
  /** The caller must reload the editor view: this buffer was already open. */
  reused: boolean;
}

/**
 * Open the bundled notices in a buffer, reusing the one a previous open left.
 *
 * The listing is generated, not written, so every open refreshes it from the
 * bundled file and drops whatever was typed into the last one. The write skips
 * the search index: hundreds of kilobytes of licence text would otherwise
 * outrank the user's own notes in every search.
 *
 * The content is written before the caller activates the tab: the editor loads
 * a buffer's text when it becomes active, so activating first would show an
 * empty document.
 */
export async function openThirdPartyNoticesBuffer(): Promise<NoticesBuffer> {
  const content = await readThirdPartyNotices();
  const existing = await findOpenNoticesBuffer();
  if (existing) {
    cancelAutosave(existing.id);
    await saveBufferContentUnindexed(existing.id, content);
    return { doc: existing, reused: true };
  }
  const doc = await bufferRegistry.createBuffer(THIRD_PARTY_NOTICES_TITLE);
  await saveBufferContentUnindexed(doc.id, content);
  return { doc, reused: false };
}
