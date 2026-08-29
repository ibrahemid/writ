import { describe, it, expect } from "vitest";

import { asSentence, formatSaveError, isChangedOnDisk } from "../../lib/save-error";

describe("formatSaveError", () => {
  it("source_changed_on_disk_maps_to_a_plain_sentence", () => {
    const message = formatSaveError(
      new Error("the file changed on disk: /Users/x/Writ/Meeting notes.md"),
    );

    expect(message).toBe(
      "This file changed outside Writ, so your changes were not saved. A copy of your version is beside it.",
    );
  });

  it("no_mapped_reason_contains_os_error_or_a_uuid", () => {
    const message = formatSaveError(
      new Error(
        "the file changed on disk: /Users/x/Writ/9f1c0f6e-3b2a-4d51-9a77-2c1f0b8e5d33.md (os error 2)",
      ),
    );

    expect(message).not.toMatch(/os error/i);
    expect(message).not.toMatch(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i);
    expect(message).not.toContain("/Users/x/Writ");
  });

  it("passes an unmapped reason through", () => {
    expect(formatSaveError(new Error("permission denied"))).toBe("permission denied");
    expect(formatSaveError("  ")).toBe("unknown error");
    expect(formatSaveError(undefined)).toBe("unknown error");
  });
});

describe("isChangedOnDisk", () => {
  it("recognizes the refusal the write guard returns", () => {
    expect(isChangedOnDisk(new Error("the file changed on disk: /Users/x/Writ/a.md"))).toBe(true);
    expect(isChangedOnDisk("the file changed on disk: /Users/x/Writ/a.md")).toBe(true);
  });

  it("does not recognize any other failure", () => {
    expect(isChangedOnDisk(new Error("io error: permission denied"))).toBe(false);
    expect(isChangedOnDisk(undefined)).toBe(false);
  });
});

describe("asSentence", () => {
  it("closes a fragment and leaves a sentence alone", () => {
    expect(asSentence("disk full")).toBe("disk full.");
    expect(asSentence(formatSaveError(new Error("the file changed on disk: /a/b.md")))).toBe(
      "This file changed outside Writ, so your changes were not saved. A copy of your version is beside it.",
    );
  });
});
