import { describe, it, expect, vi } from "vitest";
import { recheckOpenNotes, SWEEP_CONCURRENCY, type NotesSweepDeps } from "../notes-sweep";
import type { NoteDiskAnswer } from "../tauri";

function described(hash: string): NoteDiskAnswer {
  return { state: "described", disk: { hash, size: 1, mtime_ms: 0 } };
}

function tabs(...ids: string[]): NotesSweepDeps["tabs"] {
  return { activeTabs: () => ids.map((id) => ({ id })) };
}

function makeDeps(overrides: Partial<NotesSweepDeps> = {}): NotesSweepDeps {
  return {
    tabs: tabs("a", "b"),
    diskStateOf: vi.fn(async () => described("same")),
    lastKnownDiskHash: () => "same",
    onChanged: vi.fn(),
    onFailure: vi.fn(),
    ...overrides,
  };
}

describe("recheckOpenNotes", () => {
  it("asks after each open tab exactly once", async () => {
    const asked: string[] = [];
    const diskStateOf = vi.fn(async (id: string) => {
      asked.push(id);
      return described("same");
    });

    await recheckOpenNotes(makeDeps({ tabs: tabs("a", "b", "c"), diskStateOf }));

    expect(diskStateOf).toHaveBeenCalledTimes(3);
    expect(asked.sort()).toEqual(["a", "b", "c"]);
  });

  it("reads the open tabs only, never the closed-tab history", async () => {
    const history = vi.fn(() => Array.from({ length: 500 }, (_, i) => ({ id: `closed-${i}` })));
    const registry = { activeTabs: () => [{ id: "open" }], buffers: history };
    const diskStateOf = vi.fn(async () => described("same"));

    await recheckOpenNotes(makeDeps({ tabs: registry, diskStateOf }));

    expect(history).not.toHaveBeenCalled();
    expect(diskStateOf.mock.calls).toEqual([["open"]]);
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

  it("does not ask after a note Writ has not read this launch", async () => {
    // No recorded hash, so there is nothing to compare against, and the
    // question is a whole-file read and hash in Rust for nothing.
    const onChanged = vi.fn();
    const diskStateOf = vi.fn(async () => described("whatever is on disk"));
    await recheckOpenNotes(
      makeDeps({
        tabs: tabs("a"),
        diskStateOf,
        lastKnownDiskHash: () => undefined,
        onChanged,
      }),
    );

    expect(diskStateOf).not.toHaveBeenCalled();
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("says nothing about a note whose file cannot be described", async () => {
    // Gone, or not downloaded yet. Telling those apart is the write guard's
    // job, and it already fails closed.
    const onChanged = vi.fn();
    await recheckOpenNotes(
      makeDeps({
        diskStateOf: async (id) =>
          id === "a" ? { state: "undescribed" } : { state: "no_file" },
        onChanged,
      }),
    );

    expect(onChanged).not.toHaveBeenCalled();
  });

  it("checks every note even when one of them changed", async () => {
    const diskStateOf = vi.fn(async () => described("moved-on"));
    await recheckOpenNotes(makeDeps({ diskStateOf }));

    expect(diskStateOf).toHaveBeenCalledTimes(2);
  });

  it("keeps asking after one note's question fails", async () => {
    const onChanged = vi.fn();
    const onFailure = vi.fn();
    await recheckOpenNotes(
      makeDeps({
        tabs: tabs("a", "b", "c"),
        diskStateOf: async (id) => {
          if (id === "a") throw new Error("unreadable");
          return described("moved-on");
        },
        onChanged,
        onFailure,
      }),
    );

    expect(onFailure).toHaveBeenCalledWith("a");
    expect(onChanged.mock.calls.map(([payload]) => payload.bufferId).sort()).toEqual(["b", "c"]);
  });

  it("has at most a few questions in flight at once", async () => {
    let inFlight = 0;
    let most = 0;
    const diskStateOf = vi.fn(async () => {
      inFlight += 1;
      most = Math.max(most, inFlight);
      await new Promise((resolve) => setTimeout(resolve, 1));
      inFlight -= 1;
      return described("same");
    });
    const ids = Array.from({ length: 20 }, (_, i) => `n${i}`);

    await recheckOpenNotes(makeDeps({ tabs: tabs(...ids), diskStateOf }));

    expect(diskStateOf).toHaveBeenCalledTimes(20);
    expect(most).toBe(SWEEP_CONCURRENCY);
  });
});
