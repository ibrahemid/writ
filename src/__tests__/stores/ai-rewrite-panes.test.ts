import { describe, it, expect, vi, beforeEach } from "vitest";
import { createRoot, createSignal } from "solid-js";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { PaneId } from "../../lib/pane-layout";

// A rewrite with two panes (ADR-056): the tab in front is the focused pane's,
// so focus moving to a pane on another note discards a pending rewrite, and
// focus moving to a pane on the same note keeps it.

const h = vi.hoisted(() => ({
  active: null as unknown,
  aiRewrite: vi.fn(),
  aiCancel: vi.fn(),
  showToast: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  aiRewrite: (...args: unknown[]) => h.aiRewrite(...args),
  aiCancel: (...args: unknown[]) => h.aiCancel(...args),
  aiWordDiff: vi.fn().mockResolvedValue([]),
  aiHasApiKey: vi.fn().mockResolvedValue({ is_set: false, memory_only: false }),
  aiSetApiKey: vi.fn().mockResolvedValue({ is_set: true, memory_only: false }),
  aiClearApiKey: vi.fn().mockResolvedValue({ is_set: false, memory_only: false }),
  saveBufferContent: vi.fn().mockResolvedValue(null),
  setBufferViewPosition: vi.fn().mockResolvedValue(undefined),
  noteDiskState: vi.fn().mockResolvedValue({ state: "no_file" }),
}));

vi.mock("../../stores/global/notifications", () => ({
  showToast: (...args: unknown[]) => h.showToast(...args),
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => h.active },
}));

import { aiRewriteStore } from "../../stores/global/ai-rewrite";
import { createEditorStore } from "../../stores/window/editor-store";

const DISCARDED = "Rewrite discarded: the tab changed.";

function twoPanes(rightNote: string) {
  return createRoot(() => {
    const [focused, setFocused] = createSignal<PaneId>("left");
    const editor = createEditorStore({ focusedPane: focused });
    editor.forPane("left").registerView(new EditorView({ state: EditorState.create({ doc: "0123456789hello" }) }));
    editor.forPane("left").setCurrentBufferId("A");
    editor.forPane("right").registerView(new EditorView({ state: EditorState.create({ doc: "0123456789hello" }) }));
    editor.forPane("right").setCurrentBufferId(rightNote);
    h.active = { editor };
    return { setFocused };
  });
}

const range = { from: 10, to: 15, text: "hello", usedSelection: true, bufferId: "A" };

beforeEach(() => {
  h.aiRewrite.mockReset().mockResolvedValue("req-1");
  h.aiCancel.mockReset();
  h.showToast.mockReset();
  aiRewriteStore.discard();
});

describe("a pending rewrite when focus moves to the other pane", () => {
  it("is discarded when that pane shows another note", async () => {
    const { setFocused } = twoPanes("B");
    aiRewriteStore.start("proofread", range);
    expect(aiRewriteStore.status()).toBe("streaming");

    setFocused("right");

    await vi.waitFor(() => expect(aiRewriteStore.isOpen()).toBe(false));
    expect(h.showToast).toHaveBeenCalledWith(DISCARDED, "info");
    expect(h.aiCancel).toHaveBeenCalledTimes(1);
  });

  it("is kept when that pane shows the same note", async () => {
    const { setFocused } = twoPanes("A");
    aiRewriteStore.start("proofread", range);

    setFocused("right");
    await Promise.resolve();

    expect(aiRewriteStore.isOpen()).toBe(true);
    expect(aiRewriteStore.status()).toBe("streaming");
    expect(h.showToast).not.toHaveBeenCalledWith(DISCARDED, "info");
  });
});
