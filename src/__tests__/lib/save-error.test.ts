import { describe, it, expect } from "vitest";

import {
  asSentence,
  describeSaveFailure,
  formatRenameError,
  formatSaveError,
  isRetryableSaveError,
} from "../../lib/save-error";

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
    // Every code Writ mints, each carrying the worst text the layer beneath
    // could put after it: an errno, a note's id, a path, the code itself.
    const codes = [
      "ERR_FILE_CHANGED_ON_DISK",
      "ERR_FILE_NOT_DOWNLOADED",
      "ERR_NOTE_READ_ONLY",
      "ERR_PERMISSION_DENIED",
      "ERR_FILE_MISSING",
      "ERR_WRITE_TIMED_OUT",
      "ERR_WRITE_FAILED",
      "Consistency error: note has no file",
    ];

    for (const code of codes) {
      const message = formatSaveError(
        new Error(
          `${code}: /Users/x/Writ/9f1c0f6e-3b2a-4d51-9a77-2c1f0b8e5d33.md (os error 2), errno 13`,
        ),
      );

      expect(message, code).not.toMatch(/os error/i);
      expect(message, code).not.toMatch(/errno/i);
      expect(message, code).not.toMatch(
        /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i,
      );
      expect(message, code).not.toContain("/Users/x/Writ");
      expect(message, code).not.toContain("ERR_");
      expect(message.length, code).toBeGreaterThan(0);
    }
  });

  it("maps every io kind Writ mints a code for to plain words", () => {
    expect(formatSaveError(new Error("ERR_PERMISSION_DENIED: io error (os error 13)"))).toBe(
      "you do not have permission to change this file.",
    );
    expect(formatSaveError(new Error("ERR_FILE_MISSING: io error (os error 2)"))).toBe(
      "the folder this file was in is no longer there.",
    );
    expect(formatSaveError(new Error("ERR_WRITE_TIMED_OUT: io error (os error 60)"))).toBe(
      "the disk stopped responding. Check that the drive is still connected.",
    );
    expect(formatSaveError(new Error("ERR_WRITE_FAILED: io error (os error 28)"))).toBe(
      "the disk would not take the write.",
    );
    expect(formatSaveError(new Error("ERR_NOTE_READ_ONLY: note 9f1c0f6e is read-only"))).toBe(
      "this file opened read-only, so it cannot be written to.",
    );
  });

  it("says the one thing that is true for a failure with no code, rather than the raw text", () => {
    // The raw text is the operating system's or names a note by its id, and a
    // person handed either learns nothing.
    const raw = "io error: Permission denied (os error 13) on 9f1c0f6e-3b2a-4d51-9a77-2c1f0b8e5d33";
    const message = formatSaveError(new Error(raw));

    expect(message).toBe("the disk would not take the write.");
    expect(message).not.toMatch(/os error/i);
    expect(message).not.toMatch(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i);
    expect(formatSaveError("  ")).toBe("the disk would not take the write.");
    expect(formatSaveError(undefined)).toBe("the disk would not take the write.");
  });
});

describe("describeSaveFailure", () => {
  it("carries the code, the sentence and whether pressing save again could help", () => {
    expect(describeSaveFailure(new Error("ERR_PERMISSION_DENIED: io error"))).toEqual({
      code: "ERR_PERMISSION_DENIED",
      message: "you do not have permission to change this file.",
      retryable: true,
    });
  });

  it("marks a read-only note as not worth a second press", () => {
    expect(describeSaveFailure(new Error("ERR_NOTE_READ_ONLY: note x is read-only")).retryable).toBe(
      false,
    );
  });

  it("reports no code for a failure that carried none", () => {
    expect(describeSaveFailure(new Error("something the disk said")).code).toBeNull();
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

describe("formatRenameError", () => {
  it("a_changed_file_never_names_a_copy_no_rename_wrote", () => {
    // A rename carries no text of its own, so nothing is set aside. The save
    // wording promises a copy beside the note that does not exist.
    expect(formatRenameError(CHANGED)).toBe(
      "The file changed outside Writ, so it was not renamed.",
    );
    expect(formatRenameError(new Error(CHANGED))).toBe(
      "The file changed outside Writ, so it was not renamed.",
    );
  });

  it("a_file_still_downloading_maps_to_a_plain_sentence", () => {
    expect(formatRenameError(NOT_DOWNLOADED)).toBe("This file has not finished downloading yet.");
  });

  it("a_coded_failure_with_no_rename_wording_borrows_the_save_sentence", () => {
    // Every failed note operation now carries a code, so this is the ordinary
    // case rather than the odd one. A code must never reach a person as
    // itself, and the save sentence for these is true of a rename too.
    expect(formatRenameError("ERR_PERMISSION_DENIED: rename failed (os error 13)")).toBe(
      "The note could not be renamed: you do not have permission to change this file.",
    );
    expect(formatRenameError("ERR_SOMETHING_ELSE: the file is busy")).toBe(
      "The note could not be renamed.",
    );
  });

  it("the_backends_own_sentences_are_passed_through", () => {
    expect(formatRenameError("That name is empty.")).toBe("That name is empty.");
    expect(formatRenameError('A note named "Grocery list.md" is already there.')).toBe(
      'A note named "Grocery list.md" is already there.',
    );
  });

  it("a_failure_that_says_nothing_still_says_something", () => {
    expect(formatRenameError("")).toBe("The note could not be renamed.");
    expect(formatRenameError(null)).toBe("The note could not be renamed.");
  });
});
