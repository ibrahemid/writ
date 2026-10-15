import type { KatexOptions } from "katex";

/**
 * How math is typeset in the editor: the preview's choices (`throwOnError:
 * false` draws a malformed expression in place instead of failing), with
 * every command that reaches outside the expression refused. The editor's
 * window can call the app, unlike the preview's frame, so `\href`, `\url`,
 * `\includegraphics` and the `\html…` commands draw as their source.
 */
export const KATEX_OPTIONS: Readonly<KatexOptions> = {
  throwOnError: false,
  trust: false,
  strict: "ignore",
  output: "htmlAndMathml",
};

/** The one function both the worker and the page fallback typeset with. */
export function typesetMath(
  katex: { renderToString(tex: string, options?: KatexOptions): string },
  tex: string,
  display: boolean,
): string {
  return katex.renderToString(tex, { ...KATEX_OPTIONS, displayMode: display });
}

/** A request to the math worker. */
export interface MathRequest {
  id: number;
  tex: string;
  display: boolean;
}

/** The math worker's answer. */
export type MathAnswer = { id: number; markup: string } | { id: number; error: string };
