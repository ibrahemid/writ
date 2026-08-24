// Singleton state — Writ is single-window.
import { createSignal } from "solid-js";
import * as tauri from "../../services/tauri";
import type { ContentHit, FileHit, GrepOutcome, IndexStatus } from "../../types/search";

const EMPTY_STATUS: IndexStatus = {
  file_count: 0,
  truncated: false,
  has_workspace: false,
};

const [indexStatus, setIndexStatus] = createSignal<IndexStatus>(EMPTY_STATUS);
const [lastOutcome, setLastOutcome] = createSignal<GrepOutcome | null>(null);

// Highest generation seen from the backend. Rust bumps its counter per call and
// cancels the older walk, but batches already in flight still arrive; anything
// stamped below the highest generation belongs to a superseded query.
let highestGeneration = 0;

async function refreshIndexStatus(): Promise<IndexStatus> {
  try {
    const status = await tauri.workspaceIndexStatus();
    setIndexStatus(status);
    return status;
  } catch {
    // No status means no workspace rows: the palette's notice line already
    // tells the user there is nothing indexed to search.
    setIndexStatus(EMPTY_STATUS);
    return EMPTY_STATUS;
  }
}

// A failed search rejects rather than resolving empty: "no matches" and "the
// search never ran" are different answers, and the palette reports the second.
async function searchFiles(query: string): Promise<FileHit[]> {
  if (!query) return [];
  return await tauri.searchWorkspaceFiles(query);
}

export interface ContentBatch {
  hits: ContentHit[];
  outcome: GrepOutcome | null;
}

async function streamContent(
  query: string,
  onBatch: (batch: ContentBatch) => void,
  signal: AbortSignal,
): Promise<void> {
  if (!query || !indexStatus().has_workspace) return;
  setLastOutcome(null);
  try {
    await tauri.searchWorkspaceContent(query, (batch) => {
      if (signal.aborted) return;
      if (batch.generation < highestGeneration) return;
      highestGeneration = batch.generation;
      if (batch.outcome) setLastOutcome(batch.outcome);
      onBatch({ hits: batch.hits, outcome: batch.outcome });
    });
  } catch (error) {
    // An abort is this store cancelling a superseded query, not a failure.
    if (signal.aborted) return;
    throw error;
  }
}

function reset(): void {
  highestGeneration = 0;
  setIndexStatus(EMPTY_STATUS);
  setLastOutcome(null);
}

export const workspaceSearchStore = {
  indexStatus,
  lastOutcome,
  refreshIndexStatus,
  searchFiles,
  streamContent,
  reset,
};
