import { describe, it, expect } from "vitest";
import { createRoot } from "solid-js";
import { EditorView } from "@codemirror/view";
import { EditorState, EditorSelection } from "@codemirror/state";
import { search, searchPanelOpen } from "@codemirror/search";
import { createFindController, createEditorSurface, type SearchSurface } from "../../stores/global/find-store";
import { createPreviewSearchController } from "../../editor/search/preview-search-controller";

function makeView(doc: string, selection?: { from: number; to: number }) {
  const state = EditorState.create({
    doc,
    selection: selection ? EditorSelection.single(selection.from, selection.to) : undefined,
    extensions: [search({ top: true })],
  });
  const container = document.createElement("div");
  document.body.appendChild(container);
  return new EditorView({ state, parent: container });
}

function withController(view: EditorView | null, fn: (c: ReturnType<typeof createFindController>) => void) {
  createRoot((dispose) => {
    fn(createFindController(() => createEditorSurface(() => view)));
    dispose();
  });
}

function withSurface(
  surface: SearchSurface,
  fn: (c: ReturnType<typeof createFindController>) => void,
) {
  createRoot((dispose) => {
    fn(createFindController(() => surface));
    dispose();
  });
}

describe("createFindController", () => {
  it("opens with no view and stays empty", () => {
    withController(null, (find) => {
      find.open();
      expect(find.isOpen()).toBe(true);
      expect(find.matches()).toEqual({ current: 0, total: 0, capped: false });
    });
  });

  it("counts matches as the query changes", () => {
    const view = makeView("foo foo bar foo");
    withController(view, (find) => {
      find.open();
      find.setQueryText("foo");
      expect(find.matches().total).toBe(3);
      find.setQueryText("bar");
      expect(find.matches().total).toBe(1);
    });
    view.destroy();
  });

  it("seeds the query from a single-line selection on open", () => {
    const view = makeView("alpha beta gamma", { from: 6, to: 10 });
    withController(view, (find) => {
      find.open();
      expect(find.queryText()).toBe("beta");
      expect(find.matches().total).toBe(1);
    });
    view.destroy();
  });

  it("re-seeds from a new selection even when already open", () => {
    const view = makeView("alpha beta gamma", { from: 6, to: 10 });
    withController(view, (find) => {
      find.open();
      expect(find.queryText()).toBe("beta");
      view.dispatch({ selection: EditorSelection.single(11, 16) });
      find.open();
      expect(find.queryText()).toBe("gamma");
    });
    view.destroy();
  });

  it("preserves the query when reopened with no selection", () => {
    const view = makeView("alpha beta");
    withController(view, (find) => {
      find.setQueryText("alpha");
      find.open();
      expect(find.queryText()).toBe("alpha");
    });
    view.destroy();
  });

  it("seeds the find input from selection when opening replace", () => {
    const view = makeView("alpha beta gamma", { from: 0, to: 5 });
    withController(view, (find) => {
      find.showReplace();
      expect(find.queryText()).toBe("alpha");
      expect(find.replaceOpen()).toBe(true);
    });
    view.destroy();
  });

  it("advances and bumps the focus nonce on open, never opening the native panel", () => {
    const view = makeView("foo foo foo");
    withController(view, (find) => {
      const before = find.focusNonce();
      find.open();
      expect(find.focusNonce()).toBe(before + 1);
      find.setQueryText("foo");
      find.next();
      expect(find.matches().current).toBe(1);
      find.next();
      expect(find.matches().current).toBe(2);
      expect(searchPanelOpen(view.state)).toBe(false);
    });
    view.destroy();
  });

  it("does not navigate when the query is empty", () => {
    const view = makeView("foo foo");
    withController(view, (find) => {
      find.open();
      find.next();
      expect(find.matches().total).toBe(0);
      expect(searchPanelOpen(view.state)).toBe(false);
    });
    view.destroy();
  });

  it("toggles flags and re-applies the query", () => {
    const view = makeView("Foo foo FOO");
    withController(view, (find) => {
      find.open();
      find.setQueryText("foo");
      expect(find.matches().total).toBe(3);
      find.toggleCaseSensitive();
      expect(find.caseSensitive()).toBe(true);
      expect(find.matches().total).toBe(1);
    });
    view.destroy();
  });

  it("refreshes the count when the document changes under an open overlay", () => {
    const view = makeView("foo foo foo");
    withController(view, (find) => {
      find.open();
      find.setQueryText("foo");
      expect(find.matches().total).toBe(3);
      // Simulate the editor removing one occurrence while find stays open.
      view.dispatch({ changes: { from: 0, to: 4, insert: "" } });
      find.refresh();
      expect(find.matches().total).toBe(2);
    });
    view.destroy();
  });

  it("does not steal focus on find-next when already open", () => {
    const view = makeView("foo foo");
    withController(view, (find) => {
      find.open();
      find.setQueryText("foo");
      const nonce = find.focusNonce();
      find.findNextCmd();
      expect(find.focusNonce()).toBe(nonce);
      expect(find.matches().current).toBe(1);
    });
    view.destroy();
  });

  it("routes find to a preview surface: async results refresh, replace is hidden", () => {
    // A preview surface backed by the async controller. The runtime is faked:
    // posting a find immediately reports a result back through applyResult.
    const controller = createPreviewSearchController({
      post: (cmd) => {
        if (cmd.type === "find" && cmd.query) {
          controller.applyResult({ current: 1, total: 4, capped: false, ticks: [{ fraction: 0.2 }] });
        }
      },
      onUpdate: () => surfaceFind!.refresh(),
    });
    const surface: SearchSurface = {
      ...controller,
      selectionSeed: () => "",
      focus: () => {},
      canReplace: () => false,
    };
    let surfaceFind: ReturnType<typeof createFindController> | undefined;
    withSurface(surface, (find) => {
      surfaceFind = find;
      find.open();
      find.setQueryText("term");
      expect(find.matches()).toEqual({ current: 1, total: 4, capped: false });
      expect(find.ticks()).toEqual([{ fraction: 0.2 }]);
      expect(find.canReplace()).toBe(false);
    });
  });

  it("re-applies the active query to a newly-targeted surface on retarget", () => {
    // Mimic a layout flip: the active surface changes under an open overlay.
    const calls: { surface: string; query: string }[] = [];
    function fakeSurface(name: string): SearchSurface {
      return {
        setQuery: (t) => calls.push({ surface: name, query: t.query }),
        next: () => {}, previous: () => {}, replaceCurrent: () => {}, replaceAll: () => {},
        matchState: () => ({ current: 0, total: 0, capped: false }),
        matchPositions: () => [],
        clear: () => {},
        selectionSeed: () => "",
        focus: () => {},
        canReplace: () => false,
      };
    }
    let active = fakeSurface("editor");
    createRoot((dispose) => {
      const find = createFindController(() => active);
      find.open();
      find.setQueryText("needle");
      expect(calls[calls.length - 1]).toEqual({ surface: "editor", query: "needle" });
      // Flip the active surface to the preview and retarget.
      active = fakeSurface("preview");
      find.retarget();
      expect(calls[calls.length - 1]).toEqual({ surface: "preview", query: "needle" });
      dispose();
    });
  });

  it("does not re-apply on retarget when find is closed", () => {
    const calls: string[] = [];
    const surface: SearchSurface = {
      setQuery: () => calls.push("setQuery"),
      next: () => {}, previous: () => {}, replaceCurrent: () => {}, replaceAll: () => {},
      matchState: () => ({ current: 0, total: 0, capped: false }),
      matchPositions: () => [], clear: () => {}, selectionSeed: () => "", focus: () => {}, canReplace: () => true,
    };
    withSurface(surface, (find) => {
      find.retarget();
      expect(calls).toEqual([]);
    });
  });

  it("replaces all matches and clears highlights on close", () => {
    const view = makeView("foo foo foo");
    withController(view, (find) => {
      find.open();
      find.setQueryText("foo");
      find.setReplaceText("bar");
      find.replaceAll();
      expect(view.state.doc.toString()).toBe("bar bar bar");
      find.close();
      expect(find.isOpen()).toBe(false);
      expect(find.matches().total).toBe(0);
    });
    view.destroy();
  });
});
