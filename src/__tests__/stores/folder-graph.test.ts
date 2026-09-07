import { describe, it, expect, vi, beforeEach } from "vitest";
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

beforeEach(() => {
  setRoot("/notes");
  createRoot(() => {
    store = createFolderGraphStore();
  });
});

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

  it("leaves what was searched for alone while the folder stays where it is", () => {
    store.open();
    store.search("alpha");
    store.zoomOut();
    store.panLeft();

    setRoot("/notes");

    expect(store.query()).toBe("alpha");
    expect(store.zoom()).toBeLessThan(1);
    expect(store.pan()).toEqual({ x: PAN_STEP, y: 0 });
  });
});
