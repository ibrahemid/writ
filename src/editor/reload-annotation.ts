import { Annotation } from "@codemirror/state";

/**
 * Marks a transaction as text arriving from the file rather than from typing.
 *
 * The update listener reads it and skips the autosave it would otherwise
 * schedule: the document already equals the file, and writing it back would
 * defeat the cancelled autosave and move `updated_at` for nothing.
 */
export const ExternalReloadTxn = Annotation.define<boolean>();
