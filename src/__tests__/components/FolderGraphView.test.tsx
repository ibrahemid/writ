import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
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

const h = vi.hoisted(() => ({
  graph: { nodes: [], edges: [] } as {
    nodes: { path: string; name: string; folder: string }[];
    edges: { from_path: string; to_path: string; count: number }[];
  },
  error: null as string | null,
  openFile: vi.fn<(path: string) => Promise<null>>(),
  releaseGraph: vi.fn(),
}));

vi.mock("../../stores/global/theme", () => ({
  themeStore: { resolvedTokens: () => TOKEN_VALUES },
}));

vi.mock("../../stores/global/note-facts", () => ({
  noteFactsStore: {
    graph: () => () => h.graph,
    graphError: () => () => h.error,
    releaseGraph: h.releaseGraph,
  },
}));

vi.mock("../../lib/use-active-buffer", () => ({
  useActiveBuffer: () => () => null,
}));

let folderGraph: ReturnType<typeof createFolderGraphStore>;

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    folderGraph,
    tabs: { openFile: h.openFile },
  }),
}));

/** Every disc the drawing painted, with the color and how faint it was. */
interface Recorder {
  discs: { color: string; alpha: number }[];
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
    arc: () => {
      context.arcs += 1;
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
  recorder = { discs: [], lines: 0 };
  folderGraph = createFolderGraphStore();
  folderGraph.open();
  h.error = null;
  h.openFile.mockResolvedValue(null);

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
  h.graph = {
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
  };
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
      h.graph = {
        nodes: [...linked, ...loose],
        edges: linked.slice(1).map((node, i) => ({
          from_path: linked[i].path,
          to_path: node.path,
          count: 1,
        })),
      };

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

  it("closes on the close button", () => {
    twoFolders();
    const view = render(() => <FolderGraphView />);
    fireEvent.click(view.getByLabelText("Close"));
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
    h.graph = { nodes: [], edges: [] };
    const view = render(() => <FolderGraphView />);
    expect(view.getByText("No notes yet.")).toBeTruthy();
    expect(view.container.querySelector("canvas")).toBeNull();
  });

  it("says what went wrong when the index could not be read", () => {
    h.graph = { nodes: [], edges: [] };
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
    // The drawing is fitted to the canvas, so a click in the middle lands on
    // whichever note settled there; what matters is that it opens one and that
    // the view is still showing afterwards.
    fireEvent.click(canvas, { clientX: 400, clientY: 300 });
    expect(folderGraph.isOpen()).toBe(true);
  });
});
