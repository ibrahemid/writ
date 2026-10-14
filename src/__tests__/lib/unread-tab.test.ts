import { describe, it, expect } from "vitest";
import type { BufferDocument } from "../../types/buffer";
import { describeUnreadTab, unreadTabFor } from "../../lib/unread-tab";

// Which tabs restored at launch wait unread for their open, from the size Rust
// measured, and what the editor says in their place.

const MIB = 1024 * 1024;

function restored(overrides: Partial<BufferDocument> = {}): BufferDocument {
  return {
    id: "r1",
    title: "server.log",
    filename: "r1.txt",
    status: "active",
    language: null,
    source_path: "/Users/me/logs/server.log",
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
    encoding: "UTF-8",
    ...overrides,
  };
}

describe("unreadTabFor", () => {
  it("waits on a file over 50 MiB, with its size", () => {
    expect(unreadTabFor(restored({ size_bytes: 50 * MIB + 1 }))).toEqual({
      kind: "waiting",
      sizeBytes: 50 * MIB + 1,
    });
  });

  it("refuses a file over 500 MiB, read-only or not, with the reason", () => {
    for (const read_only of [false, true]) {
      expect(unreadTabFor(restored({ size_bytes: 600_000_000, read_only }))).toEqual({
        kind: "refused",
        reason: "it is 600 MB, and Writ opens files up to 524 MB.",
      });
    }
  });

  it("reads at once a file up to 50 MiB, a hex view up to 500 MiB, and a note with no file", () => {
    expect(unreadTabFor(restored({ size_bytes: 50 * MIB }))).toBeNull();
    expect(unreadTabFor(restored({ size_bytes: 500 * MIB, read_only: true }))).toBeNull();
    expect(unreadTabFor(restored({ size_bytes: 600_000_000, source_path: null }))).toBeNull();
  });
});

describe("describeUnreadTab", () => {
  it("says a file waiting on its question is not open, with its size", () => {
    expect(describeUnreadTab({ kind: "waiting", sizeBytes: 60_000_000 })).toBe(
      "This file is 60 MB and is not open.",
    );
  });

  it("says why a refused file is not open, as a failed read does", () => {
    expect(
      describeUnreadTab({ kind: "refused", reason: "it is over the 524 MB limit." }),
    ).toBe("Could not open this file: it is over the 524 MB limit.");
  });
});
