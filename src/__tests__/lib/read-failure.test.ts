import { describe, it, expect } from "vitest";
import {
  READ_FAILURE_CODES,
  canShowInFileManager,
  describeReadFailure,
  readFailureOf,
} from "../../lib/read-failure";

// A read that failed carries the code Rust put in front of its message. The
// code is the contract; nothing after it reaches a person.

describe("readFailureOf", () => {
  it("takes the code the backend put in front of the message", () => {
    for (const code of READ_FAILURE_CODES) {
      expect(readFailureOf("n1", `${code}: os error 13 for /Users/me/notes.txt`)).toEqual({
        bufferId: "n1",
        code,
      });
    }
  });

  it("reads a bare code", () => {
    expect(readFailureOf("n1", "ERR_READ_NOT_UTF8")).toEqual({
      bufferId: "n1",
      code: "ERR_READ_NOT_UTF8",
    });
  });

  it("reads the code off a thrown Error", () => {
    expect(readFailureOf("n1", new Error("ERR_READ_FILE_MISSING: gone"))).toEqual({
      bufferId: "n1",
      code: "ERR_READ_FILE_MISSING",
    });
  });

  it("answers the general code for a failure that carries none", () => {
    for (const error of [
      "stream did not contain valid UTF-8",
      new Error("boom"),
      undefined,
      null,
      42,
      "ERR_READ_NOT_UTF8X: a code that only starts like one",
    ]) {
      expect(readFailureOf("n1", error)).toEqual({ bufferId: "n1", code: "ERR_READ_FAILED" });
    }
  });
});

describe("describeReadFailure", () => {
  it("says something of its own for every code", () => {
    const sentences = READ_FAILURE_CODES.map((code) =>
      describeReadFailure({ bufferId: "n1", code }),
    );
    for (const sentence of sentences) {
      expect(sentence).toMatch(/^Could not open this file: .+\.$/);
    }
    expect(new Set(sentences).size).toBe(READ_FAILURE_CODES.length);
  });

  it("names the encoding for a file that is not UTF-8", () => {
    expect(describeReadFailure({ bufferId: "n1", code: "ERR_READ_NOT_UTF8" })).toContain(
      "not UTF-8 text",
    );
  });

  it("never renders the message that came with the code", () => {
    const failure = readFailureOf("n1", "ERR_READ_PERMISSION_DENIED: os error 13 at /secret/path");
    const sentence = describeReadFailure(failure);
    expect(sentence).not.toContain("os error");
    expect(sentence).not.toContain("/secret/path");
  });

  it("says the general sentence for a code it does not know", () => {
    expect(describeReadFailure({ bufferId: "n1", code: "ERR_SOMETHING_NEW" })).toBe(
      describeReadFailure({ bufferId: "n1", code: "ERR_READ_FAILED" }),
    );
  });
});

describe("canShowInFileManager", () => {
  it("offers the file manager for a file that is still there", () => {
    expect(canShowInFileManager({ bufferId: "n1", code: "ERR_READ_NOT_UTF8" })).toBe(true);
    expect(canShowInFileManager({ bufferId: "n1", code: "ERR_READ_FAILED" })).toBe(true);
  });

  it("does not offer it for a file that is gone", () => {
    expect(canShowInFileManager({ bufferId: "n1", code: "ERR_READ_FILE_MISSING" })).toBe(false);
  });
});
