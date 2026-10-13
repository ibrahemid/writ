import { describe, it, expect } from "vitest";
import {
  MAX_OPEN_BYTES,
  openFailureReason,
  refusalReason,
  tooLargeReason,
} from "../../lib/open-failure";

// Each reason reads as the second half of "Couldn't open <name>: ".

describe("tooLargeReason", () => {
  it("states the file's size and the size Writ opens up to", () => {
    expect(tooLargeReason(600_000_000)).toBe("it is 600 MB, and Writ opens files up to 524 MB.");
    expect(tooLargeReason(524_500_000)).toBe("it is 525 MB, and Writ opens files up to 524 MB.");
  });

  it("never states a size that reads the same as the limit it is over", () => {
    for (const size of [MAX_OPEN_BYTES + 1, 524_499_999]) {
      expect(tooLargeReason(size), String(size)).toBe("it is over the 524 MB limit.");
    }
  });

  it("is what a refusal coded as too large says", () => {
    expect(refusalReason("ERR_OPEN_TOO_LARGE: over the limit", MAX_OPEN_BYTES + 1)).toBe(
      "it is over the 524 MB limit.",
    );
  });
});

describe("openFailureReason", () => {
  it("says how to open a file the open was not allowed to read", () => {
    const error =
      "ERR_OPEN_NOT_AUTHORIZED: Writ cannot open that file from here. Use File > Open File, or drop it onto the window.";
    expect(openFailureReason(error)).toBe(
      "open it with File > Open File, or drop it onto the window.",
    );
    expect(openFailureReason(new Error(error))).toBe(
      "open it with File > Open File, or drop it onto the window.",
    );
  });

  it("says a file that is not there does not exist, with no instruction", () => {
    expect(openFailureReason("ERR_OPEN_FILE_MISSING: the file does not exist")).toBe(
      "it does not exist.",
    );
    expect(openFailureReason(new Error("ERR_OPEN_FILE_MISSING"))).toBe("it does not exist.");
  });

  it("words a read code, and anything without a code as a read that failed", () => {
    expect(openFailureReason("ERR_READ_FILE_MISSING: gone")).toBe("it is no longer there.");
    expect(openFailureReason("database is locked")).toBe("Writ could not read it.");
  });
});
