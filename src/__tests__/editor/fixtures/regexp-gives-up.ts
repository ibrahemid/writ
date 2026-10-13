import { vi } from "vitest";

/**
 * Makes `RegExp.prototype.exec` throw for one pattern what V8 throws once a
 * match attempt outgrows its backtracking stack. Restore with
 * `vi.restoreAllMocks()` or the returned spy's `mockRestore()`.
 */
export function engineGivesUpOn(sources: string[], when: (text: string) => boolean = () => true) {
  const exec = RegExp.prototype.exec;
  return vi.spyOn(RegExp.prototype, "exec").mockImplementation(function (this: RegExp, text: string) {
    if (sources.includes(this.source) && when(text)) throw new RangeError("Maximum call stack size exceeded");
    return exec.call(this, text);
  });
}
