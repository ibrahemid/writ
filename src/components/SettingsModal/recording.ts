import { createSignal } from "solid-js";

// Singleton state — Writ is single-window. The row whose recorder is waiting
// for a chord, if any: while one waits, Escape cancels the recording and the
// keys it takes never reach Settings' own handlers.
const [recordingId, setRecordingId] = createSignal<string | null>(null);

export { recordingId, setRecordingId };

export function isRecordingChord(): boolean {
  return recordingId() !== null;
}
