import { describe, it, expect, vi } from "vitest";
import {
  planExternalEdit,
  handleExternalEdit,
  readExternalEditPayload,
  type ExternalEditDeps,
} from "../external-edit";

describe("planExternalEdit", () => {
  it("ignores changes to a file with no matching buffer", () => {
    expect(planExternalEdit({ change: "modified", known: false, hasUnsaved: false })).toBe("ignore");
  });

  it("marks a tab whose file was deleted, keeping its text", () => {
    expect(planExternalEdit({ change: "removed", known: true, hasUnsaved: true })).toBe(
      "mark-removed",
    );
  });

  it("follows a moved file without asking, even with unsaved edits", () => {
    // A move changes no bytes, so putting it through the dirty gate would
    // offer to throw unsaved text away over a rename.
    expect(planExternalEdit({ change: "moved", known: true, hasUnsaved: true })).toBe("follow");
  });

  it("reloads a modified buffer that has no unsaved edits", () => {
    expect(planExternalEdit({ change: "modified", known: true, hasUnsaved: false })).toBe("reload");
  });

  it("prompts before discarding unsaved edits on a modified buffer", () => {
    expect(planExternalEdit({ change: "modified", known: true, hasUnsaved: true })).toBe("prompt");
  });

  it("reads a file at the path of a marked tab as that file coming back", () => {
    expect(
      planExternalEdit({
        change: "modified",
        known: true,
        hasUnsaved: true,
        removedOnDisk: true,
      }),
    ).toBe("returned");
  });

  it("says nothing new when a note already marked is reported gone again", () => {
    expect(
      planExternalEdit({
        change: "removed",
        known: true,
        hasUnsaved: true,
        removedOnDisk: true,
      }),
    ).toBe("ignore");
  });
});

function makeDeps(overrides: Partial<ExternalEditDeps> = {}): ExternalEditDeps {
  return {
    findBuffer: vi.fn(() => ({ id: "buf-1", title: "notes.md" })),
    hasUnsaved: vi.fn(() => false),
    reload: vi.fn(),
    cancelAutosave: vi.fn(),
    confirmReload: vi.fn(async () => true),
    isRemovedOnDisk: vi.fn(() => false),
    followMove: vi.fn(),
    markRemoved: vi.fn(),
    fileReturned: vi.fn(),
    ...overrides,
  };
}

describe("handleExternalEdit", () => {
  it("reloads the editor when there are no unsaved edits", async () => {
    const deps = makeDeps();
    await handleExternalEdit({ bufferId: "buf-1.txt", change: "modified" }, deps);
    expect(deps.reload).toHaveBeenCalledWith("buf-1");
    expect(deps.confirmReload).not.toHaveBeenCalled();
  });

  it("prompts, then reloads and drops the pending save when confirmed", async () => {
    const deps = makeDeps({
      hasUnsaved: vi.fn(() => true),
      confirmReload: vi.fn(async () => true),
    });
    await handleExternalEdit({ bufferId: "buf-1.txt", change: "modified" }, deps);
    expect(deps.confirmReload).toHaveBeenCalledWith("notes.md");
    expect(deps.cancelAutosave).toHaveBeenCalledWith("buf-1");
    expect(deps.reload).toHaveBeenCalledWith("buf-1");
  });

  it("keeps local edits and does not reload when the prompt is declined", async () => {
    const deps = makeDeps({
      hasUnsaved: vi.fn(() => true),
      confirmReload: vi.fn(async () => false),
    });
    await handleExternalEdit({ bufferId: "buf-1.txt", change: "modified" }, deps);
    expect(deps.reload).not.toHaveBeenCalled();
    expect(deps.cancelAutosave).not.toHaveBeenCalled();
  });

  it("marks the tab on deletion, never reloading", async () => {
    // The store's mark drops the queue itself, after it has taken the text
    // out of it, so nothing cancels ahead of it here (ADR-033 decision 15).
    const deps = makeDeps({ hasUnsaved: vi.fn(() => true) });
    await handleExternalEdit({ bufferId: "buf-1.txt", change: "removed" }, deps);
    expect(deps.markRemoved).toHaveBeenCalledWith("buf-1");
    expect(deps.cancelAutosave).not.toHaveBeenCalled();
    expect(deps.reload).not.toHaveBeenCalled();
    expect(deps.confirmReload).not.toHaveBeenCalled();
  });

  it("hands a file back at its own path to the store, without asking", async () => {
    const deps = makeDeps({
      isRemovedOnDisk: vi.fn(() => true),
      hasUnsaved: vi.fn(() => true),
    });
    await handleExternalEdit({ bufferId: "buf-1.txt", change: "modified" }, deps);
    expect(deps.fileReturned).toHaveBeenCalledWith("buf-1");
    expect(deps.confirmReload).not.toHaveBeenCalled();
    expect(deps.reload).not.toHaveBeenCalled();
  });

  it("says nothing twice about a note already marked", async () => {
    const deps = makeDeps({ isRemovedOnDisk: vi.fn(() => true) });
    await handleExternalEdit({ bufferId: "buf-1.txt", change: "removed" }, deps);
    expect(deps.markRemoved).not.toHaveBeenCalled();
    expect(deps.cancelAutosave).not.toHaveBeenCalled();
  });

  it("repoints the tab at where the file went", async () => {
    const deps = makeDeps({ hasUnsaved: vi.fn(() => true) });
    await handleExternalEdit(
      {
        bufferId: "buf-1.txt",
        change: "moved",
        path: "/repo/notes.md",
        newPath: "/repo/archive/notes.md",
      },
      deps,
    );
    expect(deps.followMove).toHaveBeenCalledWith("buf-1", "/repo/archive/notes.md");
    expect(deps.reload).not.toHaveBeenCalled();
    expect(deps.confirmReload).not.toHaveBeenCalled();
  });

  it("leaves the tab where it is when a move names nowhere", async () => {
    const deps = makeDeps();
    await handleExternalEdit({ bufferId: "buf-1.txt", change: "moved", newPath: null }, deps);
    expect(deps.followMove).not.toHaveBeenCalled();
  });

  it("does nothing for an unknown buffer", async () => {
    const deps = makeDeps({ findBuffer: vi.fn(() => undefined) });
    await handleExternalEdit({ bufferId: "ghost.txt", change: "modified" }, deps);
    expect(deps.reload).not.toHaveBeenCalled();
    expect(deps.markRemoved).not.toHaveBeenCalled();
  });

  it("never replaces a document that differs from its file without asking", async () => {
    // The one guarantee the watcher must not break: a folder watcher now
    // raises this for any file opened from anywhere, so a note being edited
    // while a sync client pulls its file is an ordinary Tuesday.
    const deps = makeDeps({
      hasUnsaved: vi.fn(() => true),
      confirmReload: vi.fn(async () => false),
    });

    await handleExternalEdit(
      {
        bufferId: "buf-1.txt",
        change: "modified",
        path: "/repo/notes.md",
        newPath: null,
        diskHash: "deadbeef",
      },
      deps,
    );

    expect(deps.reload).not.toHaveBeenCalled();
    expect(deps.cancelAutosave).not.toHaveBeenCalled();
  });

  it("carries what the file now holds without letting it change the decision", async () => {
    // The extra fields are for the reload and the move handling further down
    // the stack. They must not become a second input to a decision that rests
    // on the dirty predicate alone.
    const deps = makeDeps({ hasUnsaved: vi.fn(() => false) });

    await handleExternalEdit(
      {
        bufferId: "buf-1.txt",
        change: "modified",
        path: "/repo/notes.md",
        newPath: null,
        diskHash: null,
      },
      deps,
    );

    expect(deps.reload).toHaveBeenCalledWith("buf-1");
  });
});

describe("readExternalEditPayload", () => {
  it("reads a payload the way Rust serialises one", () => {
    // The field names are the contract with `WritFrontendEvent::BufferExternal`.
    // Rust named these `buffer_id` and `disk_hash` for a while; every field
    // arrived undefined, the guard dropped the event, and nothing external
    // ever reached a tab. A rename on either side has to fail here.
    expect(
      readExternalEditPayload({
        bufferId: "buf-1",
        change: "modified",
        path: "/Users/x/Writ/today.md",
        newPath: null,
        diskHash: "abc123",
      }),
    ).toEqual({
      bufferId: "buf-1",
      change: "modified",
      path: "/Users/x/Writ/today.md",
      newPath: null,
      diskHash: "abc123",
    });
  });

  it("reads a note inside the notes folder no differently from one outside it", () => {
    // Two watchers produce this event and the tab must not be able to tell
    // which. Nothing here may branch on where the file lives.
    const inside = readExternalEditPayload({
      bufferId: "buf-1",
      change: "modified",
      path: "/Users/x/Writ/today.md",
      diskHash: "abc123",
    });
    const outside = readExternalEditPayload({
      bufferId: "buf-1",
      change: "modified",
      path: "/Users/x/code/README.md",
      diskHash: "abc123",
    });

    expect(inside).toEqual({ ...outside, path: "/Users/x/Writ/today.md" });
  });

  it("rejects a payload with no buffer to act on", () => {
    expect(readExternalEditPayload({ change: "modified" })).toBeNull();
  });

  it("rejects a change it has no plan for", () => {
    expect(readExternalEditPayload({ bufferId: "buf-1", change: "renamed" })).toBeNull();
  });

  it("reads a move the way Rust serialises one", () => {
    // The words are the contract with `ExternalChange` on the Rust side:
    // `modified`, `removed`, `moved`. A rename of any of the three has to fail
    // here and in `bus_bridge`'s matching test.
    expect(
      readExternalEditPayload({
        bufferId: "buf-1",
        change: "moved",
        path: "/Users/x/Writ/today.md",
        newPath: "/Users/x/Writ/archive/today.md",
        diskHash: null,
      }),
    ).toEqual({
      bufferId: "buf-1",
      change: "moved",
      path: "/Users/x/Writ/today.md",
      newPath: "/Users/x/Writ/archive/today.md",
      diskHash: null,
    });
  });

  it("reads a removal the way Rust serialises one", () => {
    expect(
      readExternalEditPayload({
        bufferId: "buf-1",
        change: "removed",
        path: "/Users/x/Writ/today.md",
        newPath: null,
        diskHash: null,
      }),
    ).toEqual({
      bufferId: "buf-1",
      change: "removed",
      path: "/Users/x/Writ/today.md",
      newPath: null,
      diskHash: null,
    });
  });
});
