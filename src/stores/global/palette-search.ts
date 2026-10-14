import * as api from "../../services/tauri";
import { flushAutosave } from "../../services/autosave";
import type { FileHit, SearchResults } from "../../types/search";

/** The searches the palettes and the sidebar run as the person types. */
export const paletteSearch = {
  notesByName(query: string): Promise<FileHit[]> {
    return api.searchNotesByName(query);
  },
  /**
   * The one way into the notes text index (F25). Pending saves are written
   * first, and the backend reindexes every note whose deferred reindex has not
   * run yet before it answers, so text typed a moment ago is found. A failed
   * flush still searches: what is on disk is the best answer there is.
   */
  async text(query: string): Promise<SearchResults> {
    await flushAutosave().catch(() => undefined);
    return api.searchBuffers(query);
  },
};
