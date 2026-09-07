import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createRoot, createSignal } from "solid-js";
import {
  createFolderGraphStore,
  PAN_STEP,
  type FolderGraphStore,
} from "../../stores/window/folder-graph-store";

// The drawing is of the notes folder. Where that folder is comes from the
// notes store, and moving it is the one change that makes the drawing a
// drawing of somewhere else while it is on screen.

const [root, setRoot] = createSignal<string | null>("/notes");

vi.mock("../../stores/global/notes", () => ({
  notesStore: { root: () => root() },
}));

let store: FolderGraphStore;
let dispose: () => void;

beforeEach(() => {
  setRoot("/notes");
  // Held and disposed: a store left running watches the root for the rest of
  // the file, so the next test's move would reach this one's store as well.
  dispose = createRoot((disposeRoot) => {
    store = createFolderGraphStore();
    return disposeRoot;
  });
});

afterEach(() => dispose());

describe("moving the notes folder", () => {
  it("puts the search and the view back", () => {
    store.open();
    store.search("alpha");
    store.zoomOut();
    store.panLeft();

    setRoot("/elsewhere");

    expect(store.query()).toBe("");
    expect(store.zoom()).toBe(1);
    expect(store.pan()).toEqual({ x: 0, y: 0 });
  });

  it("leaves the drawing showing, on the folder that is there now", () => {
    store.open();
    setRoot("/elsewhere");
    expect(store.isOpen()).toBe(true);
  });

  it("puts them back on every move, not only on the first", () => {
    store.open();
    setRoot("/elsewhere");

    store.search("alpha");
    store.zoomOut();
    store.panLeft();
    expect(store.pan()).toEqual({ x: PAN_STEP, y: 0 });

    setRoot("/notes");

    expect(store.query()).toBe("");
    expect(store.zoom()).toBe(1);
    expect(store.pan()).toEqual({ x: 0, y: 0 });
  });
});
