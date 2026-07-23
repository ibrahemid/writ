import { describe, it, expect, vi, beforeEach } from "vitest";
import { ChangeSet } from "@codemirror/state";

const replaceRange = vi.fn();
const focusEditor = vi.fn();
const aiRewrite = vi.fn();
const aiCancel = vi.fn();
const showToast = vi.fn();

vi.mock("../../services/tauri", () => ({
  aiRewrite: (...args: unknown[]) => aiRewrite(...args),
  aiCancel: (...args: unknown[]) => aiCancel(...args),
  aiHasApiKey: vi.fn().mockResolvedValue({ is_set: false, memory_only: false }),
  aiSetApiKey: vi.fn().mockResolvedValue({ is_set: true, memory_only: false }),
  aiClearApiKey: vi.fn().mockResolvedValue({ is_set: false, memory_only: false }),
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: (...args: unknown[]) => showToast(...args),
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => ({
      editor: {
        currentBufferId: () => "buf1",
        replaceRange: (...args: unknown[]) => replaceRange(...args),
        focusEditor: () => focusEditor(),
      },
    }),
  },
}));

import { aiRewriteStore } from "../../stores/global/ai-rewrite";

const range = { from: 10, to: 15, text: "hello", usedSelection: true, bufferId: "buf1" };

describe("aiRewriteStore streaming state machine", () => {
  beforeEach(() => {
    aiRewrite.mockReset().mockResolvedValue("req-1");
    aiCancel.mockReset();
    replaceRange.mockReset();
    showToast.mockReset();
    aiRewriteStore.discard();
  });

  // The store generates the request id synchronously and passes it to
  // aiRewrite as the first argument; recover it from the mock to drive events.
  function lastRequestId(): string {
    const calls = aiRewrite.mock.calls;
    return calls[calls.length - 1]?.[0] as string;
  }

  it("accumulates chunks and applies over the anchored range in one step", async () => {
    aiRewriteStore.start("proofread", range);
    expect(aiRewrite).toHaveBeenCalledWith(expect.any(String), "proofread", "hello", undefined);
    expect(aiRewriteStore.status()).toBe("streaming");
    const id = lastRequestId();

    aiRewriteStore.handleStreamEvent({ request_id: id, kind: "chunk", text: "Hel" });
    aiRewriteStore.handleStreamEvent({ request_id: id, kind: "chunk", text: "lo" });
    expect(aiRewriteStore.result()).toBe("Hello");

    aiRewriteStore.handleStreamEvent({ request_id: id, kind: "done" });
    expect(aiRewriteStore.status()).toBe("done");

    aiRewriteStore.apply();
    expect(replaceRange).toHaveBeenCalledWith(10, 15, "Hello");
    expect(aiRewriteStore.isOpen()).toBe(false);
  });

  it("ignores events for a stale request id", () => {
    aiRewriteStore.start("polish", range);
    aiRewriteStore.handleStreamEvent({ request_id: "other", kind: "chunk", text: "x" });
    expect(aiRewriteStore.result()).toBe("");
  });

  it("surfaces an error event", () => {
    aiRewriteStore.start("rephrase", range);
    aiRewriteStore.handleStreamEvent({ request_id: lastRequestId(), kind: "error", text: "boom" });
    expect(aiRewriteStore.status()).toBe("error");
    expect(aiRewriteStore.errorMessage()).toBe("boom");
  });

  it("aborts and cancels when an edit touches the anchored range", () => {
    aiRewriteStore.start("proofread", range);
    const id = lastRequestId();
    // Replace two chars inside [10,15].
    const changes = ChangeSet.of([{ from: 12, to: 14, insert: "ZZ" }], 40);
    aiRewriteStore.onDocChanged("buf1", changes);
    expect(aiCancel).toHaveBeenCalledWith(id);
    expect(showToast).toHaveBeenCalled();
    expect(aiRewriteStore.isOpen()).toBe(false);
  });

  it("maps the anchor through an edit before the range and applies at the shifted offsets", () => {
    aiRewriteStore.start("proofread", range);
    const id = lastRequestId();
    // Insert 3 chars before the range → offsets shift by +3.
    const changes = ChangeSet.of([{ from: 0, to: 0, insert: "abc" }], 40);
    aiRewriteStore.onDocChanged("buf1", changes);
    expect(aiRewriteStore.isOpen()).toBe(true);

    aiRewriteStore.handleStreamEvent({ request_id: id, kind: "chunk", text: "HELLO" });
    aiRewriteStore.handleStreamEvent({ request_id: id, kind: "done" });
    aiRewriteStore.apply();
    expect(replaceRange).toHaveBeenCalledWith(13, 18, "HELLO");
  });

  it("holds a custom rewrite until an instruction is submitted", () => {
    aiRewriteStore.start("custom", range);
    expect(aiRewriteStore.status()).toBe("awaiting-instruction");
    expect(aiRewrite).not.toHaveBeenCalled();

    aiRewriteStore.setInstruction("make it formal");
    aiRewriteStore.submitInstruction();
    expect(aiRewrite).toHaveBeenCalledWith(expect.any(String), "custom", "hello", "make it formal");
    expect(aiRewriteStore.status()).toBe("streaming");
  });
});
