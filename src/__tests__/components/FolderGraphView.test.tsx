import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { Show, createRoot, createSignal } from "solid-js";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import FolderGraphView from "../../components/Graph/FolderGraphView";
import {
  createFolderGraphStore,
  MAX_ZOOM,
  MIN_ZOOM,
  PAN_STEP,
} from "../../stores/window/folder-graph-store";

// The whole folder, drawn. What these hold is what the view promises: a search
// dims what it does not name and drops the links between two dimmed notes, a
// folder is a color, a folder too large to draw says how much of it is shown,
// and the drawing answers the keys.

const TOKEN_VALUES: Record<string, string> = {
  "--writ-bg-canvas": "rgb(1, 1, 1)",
  "--writ-fg-muted": "rgb(2, 2, 2)",
  "--writ-fg": "rgb(3, 3, 3)",
  "--writ-accent": "#1F6F5C",
  "--writ-border": "rgb(5, 5, 5)",
  "--writ-bg-raised": "rgb(6, 6, 6)",
  "--writ-fg-faint": "rgb(7, 7, 7)",
  "--writ-icon-opacity": "0.85",
};

interface Rows {
  nodes: { path: string; name: string; folder: string }[];
  edges: { from_path: string; to_path: string; count: number }[];
}

// The rows and the open note are read through signals rather than through a
// plain object: what the folder holds changes under the drawing when a note is
// written on disk, and which note is open changes when one is chosen from the
// drawing itself, and neither may rearrange it.
const h = vi.hoisted(() => ({
  rows: null as null | (() => Rows),
  buffer: null as null | (() => { source_path: string } | null),
  error: null as string | null,
  openFile: vi.fn<(path: string) => Promise<null>>(),
  releaseGraph: vi.fn(),
  focusEditor: vi.fn(),
}));

vi.mock("../../stores/global/theme", () => ({
  themeStore: { resolvedTokens: () => TOKEN_VALUES },
}));

vi.mock("../../stores/global/note-facts", () => ({
  noteFactsStore: {
    graph: () => () => h.rows!(),
    graphError: () => () => h.error,
    releaseGraph: h.releaseGraph,
  },
}));

// The drawing is of the notes folder; where that folder is has no bearing on
// anything held here, and the store's own tests hold what a move does to it.
vi.mock("../../stores/global/notes", () => ({
  notesStore: { root: () => "/notes" },
}));

vi.mock("../../lib/use-active-buffer", () => ({
  useActiveBuffer: () => () => h.buffer!(),
}));

let folderGraph: ReturnType<typeof createFolderGraphStore>;

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    folderGraph,
    tabs: { openFile: h.openFile },
    editor: { focusEditor: h.focusEditor },
  }),
}));

const [rows, setRows] = createSignal<Rows>({ nodes: [], edges: [] });
const [buffer, setBuffer] = createSignal<{ source_path: string } | null>(null);
h.rows = rows;
h.buffer = buffer;

/** Every disc the drawing painted: where, in what color, and how faint. */
interface Recorder {
  discs: { color: string; alpha: number }[];
  spots: { x: number; y: number }[];
  lines: number;
}

let recorder: Recorder;

function stubContext(record: Recorder): CanvasRenderingContext2D {
  const context = {
    globalAlpha: 1,
    lineWidth: 1,
    font: "",
    textAlign: "start",
    textBaseline: "alphabetic",
    lineJoin: "miter",
    fillStyle: "",
    strokeStyle: "",
    setTransform: () => {},
    clearRect: () => {},
    fillRect: () => {},
    beginPath: () => {},
    moveTo: () => {},
    lineTo: () => {
      record.lines += 1;
    },
    arc: (x: number, y: number) => {
      context.arcs += 1;
      record.spots.push({ x, y });
    },
    arcs: 0,
    stroke: () => {},
    fill: () => {
      // Only a disc is filled after an arc; the ground is a fillRect.
      if (context.arcs > record.discs.length) {
        record.discs.push({ color: String(context.fillStyle), alpha: context.globalAlpha });
      }
    },
    strokeText: () => {},
    fillText: () => {},
  };
  return context as unknown as CanvasRenderingContext2D;
}

beforeEach(() => {
  recorder = { discs: [], spots: [], lines: 0 };
  folderGraph = createRoot(createFolderGraphStore);
  folderGraph.open();
  h.error = null;
  h.openFile.mockReset();
  h.releaseGraph.mockClear();
  h.focusEditor.mockClear();
  setRows({ nodes: [], edges: [] });
  setBuffer(null);
  // What the tab store does when a note is opened, as far as the drawing can
  // see it: the note becomes the active buffer.
  h.openFile.mockImplementation(async (path: string) => {
    setBuffer({ source_path: path });
    return null;
  });

  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(
    stubContext(recorder) as unknown as never,
  );
  vi.spyOn(HTMLCanvasElement.prototype, "getBoundingClientRect").mockReturnValue({
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    right: 800,
    bottom: 600,
    width: 800,
    height: 600,
    toJSON: () => ({}),
  } as DOMRect);

  const realComputed = window.getComputedStyle.bind(window);
  vi.spyOn(window, "getComputedStyle").mockImplementation(((element: Element) => {
    const stubbed =
      element instanceof HTMLCanvasElement || element.classList.contains("folder-graph");
    if (!stubbed) return realComputed(element as HTMLElement);
    return {
      fontSize: "11px",
      fontFamily: "Test Face",
      getPropertyValue: (token: string) => TOKEN_VALUES[token] ?? "",
    } as unknown as CSSStyleDeclaration;
  }) as typeof window.getComputedStyle);

  vi.stubGlobal("ResizeObserver", class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  // Frames are collected and never run: every assertion is about the frame the
  // drawing paints on the spot, not about where the settle ends up.
  vi.stubGlobal("requestAnimationFrame", () => 1);
  vi.stubGlobal("cancelAnimationFrame", () => {});
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  }));
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function twoFolders() {
  setRows({
    nodes: [
      { path: "Projects/alpha.md", name: "Alpha", folder: "Projects" },
      { path: "Projects/beta.md", name: "Beta", folder: "Projects" },
      { path: "Archive/gamma.md", name: "Gamma", folder: "Archive" },
      { path: "loose.md", name: "Loose", folder: "" },
    ],
    edges: [
      { from_path: "Projects/alpha.md", to_path: "Projects/beta.md", count: 1 },
      { from_path: "Projects/beta.md", to_path: "Archive/gamma.md", count: 1 },
      { from_path: "Archive/gamma.md", to_path: "loose.md", count: 1 },
    ],
  });
}

/**
 * A folder of `count` notes, each linked to the next.
 *
 * Enough notes that the drawing is fitted to its own span rather than to the
 * size a handful of notes is drawn at, which is where a re-fit shows.
 */
function chain(count: number) {
  const nodes = Array.from({ length: count }, (_, i) => ({
    path: `Chain/n${String(i).padStart(3, "0")}.md`,
    name: `Note ${i}`,
    folder: "Chain",
  }));
  setRows({
    nodes,
    edges: nodes.slice(1).map((node, i) => ({
      from_path: nodes[i].path,
      to_path: node.path,
      count: 1,
    })),
  });
  return nodes;
}

/** The layer, which is what the keys are pressed against. */
function layerOf(view: { container: HTMLElement }): HTMLElement {
  return view.container.querySelector(".folder-graph") as HTMLElement;
}

describe("the search", () => {
  it("dims the notes it does not name", () => {
    twoFolders();
    const view = render(() => <FolderGraphView />);
    const dim = Number(TOKEN_VALUES["--writ-icon-opacity"]);

    expect(recorder.discs.every((disc) => disc.alpha === 1)).toBe(true);

    recorder.discs = [];
    fireEvent.input(view.getByLabelText("Search notes"), { target: { value: "alpha" } });

    expect(recorder.discs.filter((disc) => disc.alpha === 1).length).toBe(1);
    expect(recorder.discs.filter((disc) => disc.alpha === dim).length).toBe(3);
    expect(view.getByText("1 of 4 notes match")).toBeTruthy();
  });

  it("leaves out a link between two notes it did not name", () => {
    twoFolders();
    const view = render(() => <FolderGraphView />);
    expect(recorder.lines).toBe(3);

    recorder.lines = 0;
    // "Beta" names one note: the two links that touch it stay, the third,
    // between two notes the search passed over, is not drawn.
    fireEvent.input(view.getByLabelText("Search notes"), { target: { value: "Beta" } });
    expect(recorder.lines).toBe(2);
  });
});

describe("the colors", () => {
  it("give two notes in one folder one color and a third folder its own", () => {
    twoFolders();
    render(() => <FolderGraphView />);

    const colors = recorder.discs.map((disc) => disc.color);
    expect(colors.length).toBe(4);
    // Two in Projects, one in Archive, one in the root: three colors, and the
    // pair in one folder painted with the same one.
    expect(new Set(colors).size).toBe(3);
    expect(colors[0]).not.toBe(colors[2]);
  });

  it("draws a note in the root of the folder in the muted foreground", () => {
    twoFolders();
    render(() => <FolderGraphView />);
    expect(recorder.discs.map((disc) => disc.color)).toContain(
      TOKEN_VALUES["--writ-fg-muted"],
    );
  });
});

describe("a folder too large to draw whole", () => {
  // The fixture is two and a half thousand notes, settled and drawn once: a
  // busy machine takes longer over that than the default allows for.
  it(
    "draws the largest linked group and says how much of the folder that is",
    { timeout: 60_000 },
    () => {
      const linked = Array.from({ length: 2100 }, (_, i) => ({
        path: `Linked/n${String(i).padStart(5, "0")}.md`,
        name: `Note ${i}`,
        folder: "Linked",
      }));
      const loose = Array.from({ length: 400 }, (_, i) => ({
        path: `Loose/l${String(i).padStart(5, "0")}.md`,
        name: `Loose ${i}`,
        folder: "Loose",
      }));
      setRows({
        nodes: [...linked, ...loose],
        edges: linked.slice(1).map((node, i) => ({
          from_path: linked[i].path,
          to_path: node.path,
          count: 1,
        })),
      });

      const view = render(() => <FolderGraphView />);
      expect(view.getByText("2000 of 2500 notes, the largest linked group")).toBeTruthy();
      expect(recorder.discs.length).toBe(2000);
    },
  );

  it("counts a folder it can draw whole without saying anything else", () => {
    twoFolders();
    const view = render(() => <FolderGraphView />);
    expect(view.getByText("4 notes")).toBeTruthy();
  });
});

describe("moving the drawing", () => {
  it("moves it with the arrows", () => {
    twoFolders();
    const view = render(() => <FolderGraphView />);
    const layer = view.container.querySelector(".folder-graph") as HTMLElement;

    fireEvent.keyDown(layer, { key: "ArrowLeft" });
    expect(folderGraph.pan()).toEqual({ x: PAN_STEP, y: 0 });
    fireEvent.keyDown(layer, { key: "ArrowDown" });
    expect(folderGraph.pan()).toEqual({ x: PAN_STEP, y: -PAN_STEP });
  });

  it("holds the zoom at both bounds", () => {
    twoFolders();
    const view = render(() => <FolderGraphView />);
    const layer = view.container.querySelector(".folder-graph") as HTMLElement;

    for (let i = 0; i < 40; i += 1) fireEvent.keyDown(layer, { key: "+" });
    expect(folderGraph.zoom()).toBe(MAX_ZOOM);

    for (let i = 0; i < 80; i += 1) fireEvent.keyDown(layer, { key: "-" });
    expect(folderGraph.zoom()).toBe(MIN_ZOOM);
  });

  it("leaves the keys to the search field while it is being typed in", () => {
    twoFolders();
    const view = render(() => <FolderGraphView />);
    fireEvent.keyDown(view.getByLabelText("Search notes"), { key: "ArrowLeft" });
    expect(folderGraph.pan()).toEqual({ x: 0, y: 0 });
  });
});

describe("closing it", () => {
  it("closes on escape", () => {
    twoFolders();
    const view = render(() => <FolderGraphView />);
    fireEvent.keyDown(view.container.querySelector(".folder-graph") as HTMLElement, {
      key: "Escape",
    });
    expect(folderGraph.isOpen()).toBe(false);
  });

  // Escape is the way out of a search as much as out of the drawing, so it is
  // read before the field is left to its own keys.
  it("closes on escape from the search field", () => {
    twoFolders();
    const view = render(() => <FolderGraphView />);
    const field = view.getByLabelText("Search notes");
    fireEvent.input(field, { target: { value: "note" } });
    fireEvent.keyDown(field, { key: "Escape" });
    expect(folderGraph.isOpen()).toBe(false);
  });

  it("closes on the close button", () => {
    twoFolders();
    const view = render(() => <FolderGraphView />);
    fireEvent.click(view.getByLabelText("Close graph"));
    expect(folderGraph.isOpen()).toBe(false);
  });

  it("hands the folder graph back when it goes", () => {
    twoFolders();
    const view = render(() => <FolderGraphView />);
    view.unmount();
    expect(h.releaseGraph).toHaveBeenCalled();
  });
});

describe("what it says when there is nothing to draw", () => {
  it("says the folder is empty rather than drawing one dot", () => {
    setRows({ nodes: [], edges: [] });
    const view = render(() => <FolderGraphView />);
    expect(view.getByText("No notes yet.")).toBeTruthy();
    expect(view.container.querySelector("canvas")).toBeNull();
  });

  it("says what went wrong when the index could not be read", () => {
    setRows({ nodes: [], edges: [] });
    h.error = "Could not read what the notes folder holds.";
    const view = render(() => <FolderGraphView />);
    expect(view.getByText("Could not read what the notes folder holds.")).toBeTruthy();
  });
});

describe("choosing a note from the drawing", () => {
  it("opens the note and stays open", () => {
    twoFolders();
    const view = render(() => <FolderGraphView />);
    const canvas = view.container.querySelector("canvas") as HTMLCanvasElement;
    // The click lands on a disc the drawing painted rather than on the middle
    // of the canvas, which the drawing owes nobody.
    const disc = recorder.spots[0];
    fireEvent.click(canvas, { clientX: disc.x, clientY: disc.y });

    expect(h.openFile).toHaveBeenCalledTimes(1);
    expect(h.openFile.mock.calls[0][0]).toMatch(/\.md$/);
    expect(folderGraph.isOpen()).toBe(true);
  });

  it("leaves every note where it settled and the drawing where it was taken", () => {
    const notes = chain(200);
    const view = render(() => <FolderGraphView />);
    const canvas = view.container.querySelector("canvas") as HTMLCanvasElement;
    const layer = layerOf(view);

    // Somewhere in a folder, which is where a note is chosen from.
    fireEvent.keyDown(layer, { key: "+" });
    fireEvent.keyDown(layer, { key: "ArrowLeft" });
    const zoom = folderGraph.zoom();
    const pan = folderGraph.pan();

    recorder.spots = [];
    fireEvent.keyDown(layer, { key: "ArrowLeft" });
    const before = [...recorder.spots];
    expect(before.length).toBe(notes.length);

    recorder.spots = [];
    fireEvent.click(canvas, { clientX: before[0].x, clientY: before[0].y });

    // The note is open, the drawing repainted to ring it, and not one disc
    // moved: the settle belongs to the folder, not to which note is open.
    expect(h.openFile).toHaveBeenCalledTimes(1);
    expect(recorder.spots).toEqual(before);
    expect(folderGraph.zoom()).toBe(zoom);
    expect(folderGraph.pan()).toEqual({ x: pan.x + PAN_STEP, y: pan.y });
  });

  it("keeps the notes that are still there when one is written on disk", () => {
    twoFolders();
    const view = render(() => <FolderGraphView />);
    const layer = layerOf(view);
    fireEvent.keyDown(layer, { key: "+" });
    fireEvent.keyDown(layer, { key: "ArrowLeft" });
    const zoom = folderGraph.zoom();
    const pan = folderGraph.pan();

    const held = rows();
    setRows({
      nodes: [...held.nodes, { path: "Projects/delta.md", name: "Delta", folder: "Projects" }],
      edges: [
        ...held.edges,
        { from_path: "Projects/alpha.md", to_path: "Projects/delta.md", count: 1 },
      ],
    });

    // The view is left where it was: the note that arrived is one more note in
    // the drawing, not a reason to send the reader back to the whole folder.
    expect(view.getByText("5 notes")).toBeTruthy();
    expect(folderGraph.zoom()).toBe(zoom);
    expect(folderGraph.pan()).toEqual(pan);

    // The new note is drawn with the four that were already there. Where those
    // four start from is `beginLayout`'s answer, held in the layout tests.
    recorder.spots = [];
    fireEvent.keyDown(layer, { key: "ArrowRight" });
    expect(recorder.spots.length).toBe(5);
  });
});

describe("what it does with focus", () => {
  // The layer is mounted the way the editor mounts it, so closing it unmounts
  // it: that is the only moment focus can be handed back.
  function open() {
    return render(() => (
      <Show when={folderGraph.isOpen()}>
        <FolderGraphView />
      </Show>
    ));
  }

  it("hands focus back to what had it", () => {
    twoFolders();
    // Stands in for the editor's own element, which is what has focus when the
    // drawing is opened from the keyboard over an open note.
    const note = document.createElement("input");
    document.body.append(note);
    note.focus();

    const view = open();
    expect(document.activeElement).toBe(layerOf(view));

    fireEvent.keyDown(layerOf(view), { key: "Escape" });
    expect(document.activeElement).toBe(note);
    expect(h.focusEditor).not.toHaveBeenCalled();
    note.remove();
  });

  it("puts focus in the editor when what had it went with the palette", () => {
    twoFolders();
    // The palette closes as the drawing opens, so what held focus is gone by
    // the time the drawing is asked to hand it back.
    const view = open();
    expect(document.activeElement).toBe(layerOf(view));

    folderGraph.close();
    expect(h.focusEditor).toHaveBeenCalledTimes(1);
  });
});
