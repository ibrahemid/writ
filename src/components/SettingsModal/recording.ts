import { createSignal } from "solid-js";

// Singleton state — Writ is single-window. The row whose recorder is waiting
// for a chord, if any: while one waits, Escape cancels the recording and the
// keys it takes never reach Settings' own handlers.
const [recordingId, setRecordingId] = createSignal<string | null>(null);

export { recordingId, setRecordingId };

/** The recording id of the window's chord row in General. */
export const WINDOW_CHORD_RECORDING = "general.hotkey.toggle";

export function isRecordingChord(): boolean {
  return recordingId() !== null;
}
