import type { BufferDocument } from "../types/buffer";
import { THRESHOLD_LARGE_BYTES } from "../editor/large-file";
import { formatBytes } from "./format-bytes";
import { MAX_OPEN_BYTES, tooLargeReason } from "./open-failure";
import { describeUnopenedFile } from "./read-failure";

/**
 * A tab the last session left open whose file Writ has not read.
 *
 * - `waiting`: the file is over the size Writ opens without asking. Showing
 *   the tab measures it again and asks; `sizeBytes` is the last size measured.
 * - `refused`: the file is over the size Writ opens. `reason` is the second
 *   half of "Could not open this file: ".
 */
export type UnreadTab =
  | { kind: "waiting"; sizeBytes: number }
  | { kind: "refused"; reason: string };

/**
 * Whether a tab restored at launch stays unread until its open answers, from
 * the size Rust measured while listing it. The tiers are writ-core's
 * `classify_size`: a read-only tab under the limit is a hex view, which opens
 * without asking, and a note with no file has nothing to measure.
 */
export function unreadTabFor(doc: BufferDocument): UnreadTab | null {
  if (doc.source_path === null) return null;
  if (doc.size_bytes > MAX_OPEN_BYTES) {
    return { kind: "refused", reason: tooLargeReason(doc.size_bytes) };
  }
  if (!doc.read_only && doc.size_bytes > THRESHOLD_LARGE_BYTES) {
    return { kind: "waiting", sizeBytes: doc.size_bytes };
  }
  return null;
}

/** What the editor says in place of an unread tab's file. */
export function describeUnreadTab(tab: UnreadTab): string {
  if (tab.kind === "refused") return describeUnopenedFile(tab.reason);
  return `This file is ${formatBytes(tab.sizeBytes)} and is not open.`;
}
