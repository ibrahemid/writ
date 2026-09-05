import { describe, it, expect, vi } from "vitest";
import { recheckOpenNotes, type NotesSweepDeps } from "../notes-sweep";
import type { NoteDiskAnswer } from "../tauri";

function described(hash: string): NoteDiskAnswer {
  return { state: "described", disk: { hash, size: 1, mtime_ms: 0 } };
}

function makeDeps(overrides: Partial<NotesSweepDeps> = {}): NotesSweepDeps {
  return {
    openNotes: () => [{ id: "a" }, { id: "b" }],
    diskStateOf: vi.fn(async () => described("same")),
    lastKnownDiskHash: () => "same",
    onChanged: vi.fn(),
    ...overrides,
  };
}

describe("recheckOpenNotes", () => {
  it("asks after each open note exactly once", async () => {
    const asked: string[] = [];
    const diskStateOf = vi.fn(async (id: string) => {
      asked.push(id);
      return described("same");
    });
    const deps = makeDeps({
      openNotes: () => [{ id: "a" }, { id: "b" }, { id: "c" }],
      diskStateOf,
    });

    await recheckOpenNotes(deps);

    expect(diskStateOf).toHaveBeenCalledTimes(3);
    expect(asked).toEqual(["a", "b", "c"]);
  });

  it("says nothing about a note whose file still holds what Writ recorded", async () => {
    const onChanged = vi.fn();
    await recheckOpenNotes(makeDeps({ onChanged }));

    expect(onChanged).not.toHaveBeenCalled();
  });

  it("routes a note whose file moved on through the external-change path", async () => {
    const onChanged = vi.fn();
    await recheckOpenNotes(
      makeDeps({
        openNotes: () => [{ id: "a" }, { id: "b" }],
        diskStateOf: async (id) => described(id === "b" ? "moved-on" : "same"),
        onChanged,
      }),
    );

    expect(onChanged).toHaveBeenCalledTimes(1);
    expect(onChanged).toHaveBeenCalledWith({
      bufferId: "b",
      change: "modified",
      diskHash: "moved-on",
    });
  });

  it("says nothing about a note Writ has not read this launch", async () => {
    // No recorded hash, so there is nothing to compare against. Claiming a
    // change would put a discard-your-work prompt over a document nobody has
    // typed into; the tab reads its file when it mounts.
    const onChanged = vi.fn();
    await recheckOpenNotes(
      makeDeps({
        openNotes: () => [{ id: "a" }],
        diskStateOf: async () => described("whatever is on disk"),
        lastKnownDiskHash: () => undefined,
        onChanged,
      }),
    );

    expect(onChanged).not.toHaveBeenCalled();
  });

  it("says nothing about a note whose file cannot be described", async () => {
    // Gone, or not downloaded yet. Telling those apart is the write guard's
    // job, and it already fails closed.
    const onChanged = vi.fn();
    await recheckOpenNotes(
      makeDeps({
        openNotes: () => [{ id: "a" }, { id: "b" }],
        diskStateOf: async (id) =>
          id === "a" ? { state: "undescribed" } : { state: "no_file" },
        onChanged,
      }),
    );

    expect(onChanged).not.toHaveBeenCalled();
  });

  it("checks every note even when one of them changed", async () => {
    const diskStateOf = vi.fn(async () => described("moved-on"));
    await recheckOpenNotes(
      makeDeps({ openNotes: () => [{ id: "a" }, { id: "b" }], diskStateOf }),
    );

    expect(diskStateOf).toHaveBeenCalledTimes(2);
  });
});
