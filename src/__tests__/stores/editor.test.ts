import { describe, it, expect, vi, beforeEach } from "vitest";
import { createEditorStore, type EditorStore } from "../../stores/window/editor-store";

describe("editor-store (per-window factory)", () => {
  let editorStore: EditorStore;

  beforeEach(() => {
    editorStore = createEditorStore();
  });

  describe("cursor signals", () => {
    it("tracks cursor line", () => {
      editorStore.setCursorLine(42);
      expect(editorStore.cursorLine()).toBe(42);
    });

    it("tracks cursor column", () => {
      editorStore.setCursorCol(15);
      expect(editorStore.cursorCol()).toBe(15);
    });
  });

  describe("line count", () => {
    it("tracks document line count", () => {
      editorStore.setLineCount(200);
      expect(editorStore.lineCount()).toBe(200);
    });
  });

  describe("reveal requests", () => {
    it("carries the buffer id and line, with a fresh seq each time", () => {
      expect(editorStore.pendingReveal()).toBeNull();

      editorStore.requestReveal("buf-1", 12);
      const first = editorStore.pendingReveal();
      expect(first).toMatchObject({ bufferId: "buf-1", line: 12 });

      editorStore.requestReveal("buf-1", 12);
      const second = editorStore.pendingReveal();
      expect(second!.seq).toBeGreaterThan(first!.seq);
    });

    it("clearReveal drops the pending request after it is applied", () => {
      editorStore.requestReveal("buf-1", 3);
      expect(editorStore.pendingReveal()).not.toBeNull();
      editorStore.clearReveal();
      expect(editorStore.pendingReveal()).toBeNull();
    });
  });

  describe("language", () => {
    it("defaults to null", () => {
      expect(editorStore.language()).toBeNull();
    });

    it("tracks detected language", () => {
      editorStore.setLanguage("rust");
      expect(editorStore.language()).toBe("rust");
    });

    it("can be reset to null", () => {
      editorStore.setLanguage("python");
      editorStore.setLanguage(null);
      expect(editorStore.language()).toBeNull();
    });
  });

  describe("external reload request (#53.4)", () => {
    it("defaults to no pending reload", () => {
      expect(editorStore.externalReload()).toBeNull();
    });

    it("publishes the buffer id to reload", () => {
      editorStore.requestExternalReload("buf-7");
      expect(editorStore.externalReload()?.id).toBe("buf-7");
    });

    it("re-fires for repeated external edits to the same buffer via an incrementing seq", () => {
      editorStore.requestExternalReload("buf-7");
      const first = editorStore.externalReload()!.seq;
      editorStore.requestExternalReload("buf-7");
      const second = editorStore.externalReload()!.seq;
      expect(second).toBeGreaterThan(first);
    });
  });

  describe("selection count", () => {
    it("defaults to 1", () => {
      expect(editorStore.selectionCount()).toBe(1);
    });

    it("tracks multi-cursor count", () => {
      editorStore.setSelectionCount(5);
      expect(editorStore.selectionCount()).toBe(5);
    });
  });

  describe("view registration", () => {
    it("focusEditor does not throw when no view registered", () => {
      expect(() => editorStore.focusEditor()).not.toThrow();
    });

    it("focusEditor calls focus on registered view", () => {
      const mockView = { focus: vi.fn() } as unknown as import("@codemirror/view").EditorView;
      editorStore.registerView(mockView);

      editorStore.focusEditor();

      expect(mockView.focus).toHaveBeenCalledOnce();
    });

    it("focusEditor does nothing after view is unregistered", () => {
      const mockView = { focus: vi.fn() } as unknown as import("@codemirror/view").EditorView;
      editorStore.registerView(mockView);
      editorStore.registerView(null);

      editorStore.focusEditor();

      expect(mockView.focus).not.toHaveBeenCalled();
    });
  });

  describe("current buffer id (preview render gate, #97)", () => {
    it("defaults to null", () => {
      expect(editorStore.currentBufferId()).toBeNull();
    });

    it("tracks the loaded buffer id and can be cleared", () => {
      editorStore.setCurrentBufferId("buf-1");
      expect(editorStore.currentBufferId()).toBe("buf-1");
      editorStore.setCurrentBufferId(null);
      expect(editorStore.currentBufferId()).toBeNull();
    });
  });

  describe("per-window isolation", () => {
    it("two instances are independent", () => {
      const a = createEditorStore();
      const b = createEditorStore();
      a.setCursorLine(10);
      b.setCursorLine(99);
      expect(a.cursorLine()).toBe(10);
      expect(b.cursorLine()).toBe(99);
    });
  });
});
