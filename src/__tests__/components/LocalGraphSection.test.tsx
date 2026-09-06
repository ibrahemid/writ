import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import LocalGraphSection from "../../components/RightPanel/LocalGraphSection";

// The panel's drawing. A note with nothing around it renders no section at
// all, and the section that does render names a note on hover and opens it on
// a click.

const TOKEN_VALUES: Record<string, string> = {
  "--writ-bg-canvas": "rgb(1, 1, 1)",
  "--writ-fg-muted": "rgb(2, 2, 2)",
  "--writ-fg": "rgb(3, 3, 3)",
  "--writ-accent": "rgb(4, 4, 4)",
  "--writ-border": "rgb(5, 5, 5)",
  "--writ-bg-raised": "rgb(6, 6, 6)",
};

const h = vi.hoisted(() => ({
  graph: { nodes: [], edges: [] } as {
    nodes: { path: string; name: string; folder: string }[];
    edges: { from_path: string; to_path: string; count: number }[];
  },
  openFile: vi.fn<(path: string) => Promise<{ id: string } | null>>(),
  holdGraph: vi.fn(),
  releaseGraph: vi.fn(),
  collapsed: new Set<string>(),
}));

vi.mock("../../stores/global/theme", () => ({
  themeStore: { resolvedTokens: () => TOKEN_VALUES },
}));

vi.mock("../../stores/global/note-facts", () => ({
  noteFactsStore: {
    graph: () => {
      h.holdGraph();
      return () => h.graph;
    },
    releaseGraph: h.releaseGraph,
  },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    rightPanel: {
      isCollapsed: (section: string) => h.collapsed.has(section),
      toggleSection: () => {},
    },
    tabs: { openFile: h.openFile },
  }),
}));

const FOLDER = {
  nodes: [
    { path: "Alpha.md", name: "Alpha", folder: "" },
    { path: "Beta.md", name: "Beta", folder: "" },
    { path: "Gamma.md", name: "Gamma", folder: "" },
    { path: "Elsewhere.md", name: "Elsewhere", folder: "" },
    { path: "Nothing.md", name: "Nothing", folder: "" },
  ],
  edges: [
    { from_path: "Alpha.md", to_path: "Beta.md", count: 1 },
    { from_path: "Gamma.md", to_path: "Alpha.md", count: 2 },
    { from_path: "Beta.md", to_path: "Gamma.md", count: 1 },
    { from_path: "Elsewhere.md", to_path: "Beta.md", count: 1 },
  ],
};

function stubContext(): CanvasRenderingContext2D {
  return {
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
    lineTo: () => {},
    arc: () => {},
    stroke: () => {},
    fill: () => {},
    strokeText: () => {},
    fillText: () => {},
  } as unknown as CanvasRenderingContext2D;
}

beforeEach(() => {
  h.graph = FOLDER;
  h.openFile.mockReset();
  h.holdGraph.mockReset();
  h.releaseGraph.mockReset();
  h.openFile.mockResolvedValue({ id: "buf-2" });
  h.collapsed = new Set();

  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(
    stubContext() as unknown as never,
  );
  vi.spyOn(HTMLCanvasElement.prototype, "getBoundingClientRect").mockReturnValue({
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    right: 216,
    bottom: 160,
    width: 216,
    height: 160,
    toJSON: () => ({}),
  } as DOMRect);

  const realComputed = window.getComputedStyle.bind(window);
  vi.spyOn(window, "getComputedStyle").mockImplementation(((element: Element) => {
    if (!(element instanceof HTMLCanvasElement)) return realComputed(element as HTMLElement);
    return {
      fontSize: "11px",
      fontFamily: "Test Face",
      getPropertyValue: (token: string) => TOKEN_VALUES[token] ?? "",
    } as unknown as CSSStyleDeclaration;
  }) as typeof window.getComputedStyle);

  // Settling at once puts every note where it ends up before the first hover,
  // so a pointer test never races a frame.
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: query.includes("prefers-reduced-motion"),
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

/** The first point of the canvas that is over a note other than the open one. */
function findNeighbour(canvas: HTMLCanvasElement, line: HTMLElement): { x: number; y: number } {
  for (let y = 0; y <= 160; y += 2) {
    for (let x = 0; x <= 216; x += 2) {
      fireEvent.pointerMove(canvas, { clientX: x, clientY: y });
      const name = line.textContent ?? "";
      if (name !== "" && name !== "Alpha") return { x, y };
    }
  }
  throw new Error("no note under any point of the drawing");
}

describe("the section beside a note with nothing around it", () => {
  it("renders nothing at all, not an empty heading", () => {
    const { queryByText, queryByRole, container } = render(() => (
      <LocalGraphSection path="Nothing.md" />
    ));
    expect(queryByText("Nearby notes")).toBeNull();
    expect(queryByRole("img")).toBeNull();
    expect(container.querySelector("canvas")).toBeNull();
    expect(container.querySelector(".right-panel-section")).toBeNull();
  });
});

describe("what the section holds while it is showing", () => {
  it("hands the folder graph back when it goes away, and takes it again", () => {
    const first = render(() => <LocalGraphSection path="Alpha.md" />);
    expect(h.holdGraph).toHaveBeenCalledTimes(1);
    expect(h.releaseGraph).not.toHaveBeenCalled();

    first.unmount();
    expect(h.releaseGraph).toHaveBeenCalledTimes(1);

    const again = render(() => <LocalGraphSection path="Alpha.md" />);
    expect(h.holdGraph).toHaveBeenCalledTimes(2);
    again.unmount();
    expect(h.releaseGraph).toHaveBeenCalledTimes(2);
  });

  it("hands it back even when it drew nothing", () => {
    const { unmount } = render(() => <LocalGraphSection path="Nothing.md" />);
    expect(h.holdGraph).toHaveBeenCalledTimes(1);
    unmount();
    expect(h.releaseGraph).toHaveBeenCalledTimes(1);
  });
});

describe("the section beside a note with notes around it", () => {
  it("renders the drawing under its heading", () => {
    const { getByText, getByRole } = render(() => <LocalGraphSection path="Alpha.md" />);
    expect(getByText("Nearby notes")).toBeTruthy();
    expect(getByRole("img").getAttribute("aria-label")).toBe("Alpha and 2 notes it links with");
  });

  it("names the note the pointer is over, and nothing once it leaves", () => {
    const { container } = render(() => <LocalGraphSection path="Alpha.md" />);
    const canvas = container.querySelector("canvas") as HTMLCanvasElement;
    const line = container.querySelector(".graph-hovered") as HTMLElement;
    expect(line.textContent).toBe("");

    findNeighbour(canvas, line);
    expect(["Beta", "Gamma"]).toContain(line.textContent);

    fireEvent.pointerLeave(canvas);
    expect(line.textContent).toBe("");
  });

  it("opens the note that was named", () => {
    const { container } = render(() => <LocalGraphSection path="Alpha.md" />);
    const canvas = container.querySelector("canvas") as HTMLCanvasElement;
    const line = container.querySelector(".graph-hovered") as HTMLElement;

    const point = findNeighbour(canvas, line);
    const named = line.textContent ?? "";
    fireEvent.click(canvas, { clientX: point.x, clientY: point.y });

    expect(h.openFile).toHaveBeenCalledTimes(1);
    expect(h.openFile).toHaveBeenCalledWith(`${named}.md`);
  });
});
