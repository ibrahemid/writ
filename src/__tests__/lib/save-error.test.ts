import { describe, it, expect } from "vitest";

import { asSentence, formatSaveError, isRetryableSaveError } from "../../lib/save-error";

const CHANGED = "ERR_FILE_CHANGED_ON_DISK: the file changed on disk: /Users/x/Writ/Meeting notes.md";
const NOT_DOWNLOADED =
  "ERR_FILE_NOT_DOWNLOADED: the file has not finished downloading: /Users/x/Writ/Evicted.md";

describe("formatSaveError", () => {
  it("source_changed_on_disk_maps_to_a_plain_sentence", () => {
    expect(formatSaveError(new Error(CHANGED))).toBe(
      "the file changed outside Writ. A copy of your version is beside it.",
    );
  });

  it("a_file_still_downloading_maps_to_a_plain_sentence", () => {
    expect(formatSaveError(new Error(NOT_DOWNLOADED))).toBe(
      "this file has not finished downloading, so your changes were not saved yet.",
    );
  });

  it("reads the code rather than the message after it", () => {
    expect(formatSaveError("ERR_FILE_CHANGED_ON_DISK: whatever a later build logs here")).toBe(
      "the file changed outside Writ. A copy of your version is beside it.",
    );
  });

  it("no_mapped_reason_contains_os_error_or_a_uuid", () => {
    const message = formatSaveError(
      new Error(
        "ERR_FILE_CHANGED_ON_DISK: the file changed on disk: /Users/x/Writ/9f1c0f6e-3b2a-4d51-9a77-2c1f0b8e5d33.md (os error 2)",
      ),
    );

    expect(message).not.toMatch(/os error/i);
    expect(message).not.toMatch(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i);
    expect(message).not.toContain("/Users/x/Writ");
    expect(message).not.toContain("ERR_");
  });

  it("passes an unmapped reason through", () => {
    expect(formatSaveError(new Error("permission denied"))).toBe("permission denied");
    expect(formatSaveError("  ")).toBe("unknown error");
    expect(formatSaveError(undefined)).toBe("unknown error");
  });
});

describe("isRetryableSaveError", () => {
  it("says no to the two a repeat cannot fix", () => {
    expect(isRetryableSaveError(new Error(CHANGED))).toBe(false);
    expect(isRetryableSaveError(NOT_DOWNLOADED)).toBe(false);
  });

  it("says yes to every other failure", () => {
    expect(isRetryableSaveError(new Error("io error: permission denied"))).toBe(true);
    expect(isRetryableSaveError(undefined)).toBe(true);
  });
});

describe("asSentence", () => {
  it("closes a fragment and leaves a sentence alone", () => {
    expect(asSentence("disk full")).toBe("disk full.");
    expect(asSentence(formatSaveError(new Error(CHANGED)))).toBe(
      "the file changed outside Writ. A copy of your version is beside it.",
    );
  });
});
