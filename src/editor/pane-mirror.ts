import { Annotation, Transaction, type ChangeSet, type TransactionSpec } from "@codemirror/state";
import { ExternalReloadTxn } from "./external-reload";

/**
 * Marks a change carried from the other pane's editor of the same note
 * (ADR-056). The pane that made the change recorded it, queued its save and
 * told the rewrite; the one taking it only shows it.
 */
export const MirroredTxn = Annotation.define<boolean>();

/**
 * The transaction that carries `changes` into the other pane: outside its
 * undo history, so each pane undoes its own typing, and still a reload when
 * the change came from the file, so it queues no save there either.
 */
export function mirroredSpec(changes: ChangeSet, fromFile: boolean): TransactionSpec {
  return {
    changes,
    annotations: [
      MirroredTxn.of(true),
      Transaction.addToHistory.of(false),
      ...(fromFile ? [ExternalReloadTxn.of(true)] : []),
    ],
  };
}
