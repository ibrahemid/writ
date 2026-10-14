import { describe, it, expect } from "vitest";
import { userErrorText } from "../../lib/error-text";

describe("userErrorText", () => {
  it("passes on a refusal Rust wrote for a person", () => {
    expect(userErrorText("Could not open the link.")).toBe("Could not open the link.");
    expect(userErrorText(new Error("Could not reach the local model server. Is Ollama running?"))).toBe(
      "Could not reach the local model server. Is Ollama running?",
    );
    expect(userErrorText("“Plan.md” is already in that folder.")).toBe(
      "“Plan.md” is already in that folder.",
    );
  });

  it("holds back the Display text of writ-core's and writ-storage's errors", () => {
    for (const display of [
      "buffer not found: 3f2a9c1e",
      "consistency error: buffer not found: 3f2a9c1e",
      "io error: Permission denied (os error 13)",
      "database error: database is locked",
      "version 12 is not in the store",
      "the name Plan.md is already taken in /Users/a/Writ",
    ]) {
      expect(userErrorText(display), display).toBeNull();
      expect(userErrorText(new Error(display)), display).toBeNull();
    }
  });

  it("holds back raw OS text, error codes and size units", () => {
    expect(userErrorText("Permission denied (os error 13)")).toBeNull();
    expect(userErrorText("EACCES: permission denied, rename '/home'")).toBeNull();
    expect(userErrorText("ERR_READ_FAILED: Is a directory.")).toBeNull();
    expect(userErrorText("File is 60.0 MiB — files larger than 50.0 MiB are not opened.")).toBeNull();
    expect(userErrorText("The buffer is gone.")).toBeNull();
  });

  it("has nothing to say for an empty or unknown rejection", () => {
    expect(userErrorText("")).toBeNull();
    expect(userErrorText("   ")).toBeNull();
    expect(userErrorText(undefined)).toBeNull();
    expect(userErrorText({ code: 3 })).toBeNull();
  });
});
