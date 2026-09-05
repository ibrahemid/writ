import { bufferRegistry } from "../stores/global/buffer-registry";

/** The name a person knows the note by: its file name, else the tab title. */
export function noteName(id: string): string {
  const doc = bufferRegistry.buffers().find((b) => b.id === id);
  if (!doc) return "this note";
  return doc.source_path?.split(/[\\/]/).pop() || doc.title;
}
