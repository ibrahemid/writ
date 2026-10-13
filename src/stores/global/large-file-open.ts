import { batch, createRoot, createSignal } from "solid-js";

/**
 * A file over the size Writ opens without asking, from the question to the
 * moment the editor shows it.
 *
 * `asking` states the size before anything reads the file. `opening` covers
 * the confirmed open and the editor's first read of the text, which on a
 * 500 MB file take seconds; `bufferId` is set once the open has answered with
 * the file's row.
 */
export type LargeFileOpenState =
  | { phase: "asking"; name: string; sizeBytes: number }
  | { phase: "opening"; name: string; sizeBytes: number; bufferId: string | null };

// Resolves after the browser has painted the frame now pending. The confirmed
// open and the editor's read are synchronous commands that hold the main
// thread, so the progress has to be on screen before either starts.
function afterNextPaint(): Promise<void> {
  return new Promise((resolve) => {
    if (typeof requestAnimationFrame !== "function") {
      setTimeout(resolve, 0);
      return;
    }
    requestAnimationFrame(() => setTimeout(resolve, 0));
  });
}

interface Question {
  name: string;
  sizeBytes: number;
  answer: (open: boolean) => void;
}

function createLargeFileOpen() {
  // Singleton state — Writ is single-window
  const [state, setState] = createSignal<LargeFileOpenState | null>(null);
  // The question on screen answers through `answer`. One asked while a
  // question or its progress is up waits here, in the order it was asked.
  let answer: ((open: boolean) => void) | null = null;
  const waiting: Question[] = [];

  function show(question: Question): void {
    answer = question.answer;
    setState({ phase: "asking", name: question.name, sizeBytes: question.sizeBytes });
  }

  // The dialog is free again, and the question that waited longest takes it,
  // in one update so the dialog stays up between the two.
  function release(): void {
    batch(() => {
      setState(null);
      const next = waiting.shift();
      if (next) show(next);
    });
  }

  /**
   * Asks whether to open `name`, which is `sizeBytes` long. Resolves true for
   * Open. While another question or its progress is up, this one waits its
   * turn: only the person answers a question.
   */
  function ask(name: string, sizeBytes: number): Promise<boolean> {
    return new Promise<boolean>((resolve) => {
      const question = { name, sizeBytes, answer: resolve };
      if (state() === null) show(question);
      else waiting.push(question);
    });
  }

  /** The person's answer. Open turns the question into the progress. */
  function respond(open: boolean): void {
    const current = state();
    if (current?.phase !== "asking") return;
    const resolve = answer;
    answer = null;
    if (open) {
      setState({
        phase: "opening",
        name: current.name,
        sizeBytes: current.sizeBytes,
        bufferId: null,
      });
    } else {
      release();
    }
    resolve?.(open);
  }

  /** Resolves once the progress is on screen. */
  function shown(): Promise<void> {
    return afterNextPaint();
  }

  /** The open answered with this row; the progress lasts until the editor shows it. */
  function handOff(bufferId: string): void {
    const current = state();
    if (current?.phase !== "opening") return;
    setState({ ...current, bufferId });
  }

  /** Ends the progress for this row, if it is the one in progress. */
  function finishFor(bufferId: string): void {
    const current = state();
    if (current?.phase === "opening" && current.bufferId === bufferId) release();
  }

  /** Ends the progress, whatever it was for. */
  function finish(): void {
    if (state()?.phase === "opening") release();
  }

  return { state, ask, respond, shown, handOff, finishFor, finish };
}

export const largeFileOpen = createRoot(createLargeFileOpen);
