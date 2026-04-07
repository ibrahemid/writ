import { describe, it, expect, vi, beforeEach } from "vitest";
import { editorStore } from "../../stores/editor";

describe("editorStore", () => {
  beforeEach(() => {
    editorStore.setCursorLine(1);
    editorStore.setCursorCol(1);
    editorStore.setLineCount(0);
    editorStore.setLanguage(null);
    editorStore.setSelectionCount(1);
    editorStore.registerView(null);
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
});
