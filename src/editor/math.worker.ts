// Typesets math off the page's thread: KaTeX builds its markup as a string,
// so a worker needs no document to run it.
import katex from "katex";
import { typesetMath, type MathAnswer, type MathRequest } from "./math-render";

interface WorkerScope {
  onmessage: ((event: MessageEvent<MathRequest>) => void) | null;
  postMessage(answer: MathAnswer): void;
}

const scope = self as unknown as WorkerScope;

scope.onmessage = (event) => {
  const { id, tex, display } = event.data;
  try {
    scope.postMessage({ id, markup: typesetMath(katex, tex, display) });
  } catch (error) {
    scope.postMessage({ id, error: error instanceof Error ? error.message : String(error) });
  }
};
