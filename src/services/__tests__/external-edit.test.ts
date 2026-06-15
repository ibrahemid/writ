import { describe, it, expect, vi } from "vitest";
import {
  planExternalEdit,
  handleExternalEdit,
  type ExternalEditDeps,
} from "../external-edit";

describe("planExternalEdit", () => {
  it("ignores changes to a file with no matching buffer", () => {
    expect(planExternalEdit({ change: "modified", known: false, hasUnsaved: false })).toBe("ignore");
  });

  it("only toasts on deletion (the buffer keeps its content)", () => {
    expect(planExternalEdit({ change: "deleted", known: true, hasUnsaved: true })).toBe("toast");
  });

  it("reloads a modified buffer that has no unsaved edits", () => {
    expect(planExternalEdit({ change: "modified", known: true, hasUnsaved: false })).toBe("reload");
  });

  it("prompts before discarding unsaved edits on a modified buffer", () => {
    expect(planExternalEdit({ change: "modified", known: true, hasUnsaved: true })).toBe("prompt");
  });
});

function makeDeps(overrides: Partial<ExternalEditDeps> = {}): ExternalEditDeps {
  return {
    findBuffer: vi.fn(() => ({ id: "buf-1", title: "notes.md" })),
    hasUnsaved: vi.fn(() => false),
    reload: vi.fn(),
    cancelAutosave: vi.fn(),
    toast: vi.fn(),
    confirmReload: vi.fn(async () => true),
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

  it("only toasts on deletion, never reloading", async () => {
    const deps = makeDeps();
    await handleExternalEdit({ bufferId: "buf-1.txt", change: "deleted" }, deps);
    expect(deps.toast).toHaveBeenCalledWith('File "notes.md" deleted externally', "warning");
    expect(deps.reload).not.toHaveBeenCalled();
  });

  it("does nothing for an unknown buffer", async () => {
    const deps = makeDeps({ findBuffer: vi.fn(() => undefined) });
    await handleExternalEdit({ bufferId: "ghost.txt", change: "modified" }, deps);
    expect(deps.reload).not.toHaveBeenCalled();
    expect(deps.toast).not.toHaveBeenCalled();
  });
});
