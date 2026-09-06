import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createSignal } from "solid-js";
import { render, cleanup } from "@solidjs/testing-library";
import GraphCanvas from "../../components/Graph/GraphCanvas";

// The drawing takes every colour it paints with from a token read off the
// canvas element, so a theme it has never seen still paints. The sentinels
// below stand in for the token values: nothing the component could have
// hard-coded can pass for one.

const TOKEN_VALUES: Record<string, string> = {
  "--writ-bg-canvas": "rgb(1, 1, 1)",
  "--writ-fg-muted": "rgb(2, 2, 2)",
  "--writ-fg": "rgb(3, 3, 3)",
  "--writ-accent": "rgb(4, 4, 4)",
  "--writ-border": "rgb(5, 5, 5)",
  "--writ-bg-raised": "rgb(6, 6, 6)",
};

// The store writes its signal before it writes the custom properties to the
// root, so the drawing has to read the palette after the whole change lands,
// not the moment the signal moves. The signal here is what a theme change is.
const [themeChanged, setThemeChanged] = createSignal(0);

vi.mock("../../stores/global/theme", () => ({
  themeStore: {
    resolvedTokens: () => {
      themeChanged();
      return TOKEN_VALUES;
    },
  },
}));

const NODES = [
  { path: "Alpha.md", name: "Alpha", degree: 2 },
  { path: "Beta.md", name: "Beta", degree: 1 },
  { path: "Gamma.md", name: "Gamma", degree: 1 },
];

const EDGES = [
  { from: "Alpha.md", to: "Beta.md" },
  { from: "Alpha.md", to: "Gamma.md" },
];

interface Recorder {
  fills: string[];
  strokes: string[];
  paints: number;
  asked: string[];
}

let recorder: Recorder;
let observers: StubResizeObserver[];
let frames: { id: number; run: FrameRequestCallback }[];
let cancelled: number[];
let reducedMotion: boolean;

class StubResizeObserver {
  observed: Element[] = [];
  disconnected = 0;
  constructor(readonly run: ResizeObserverCallback) {
    observers.push(this);
  }
  observe(target: Element) {
    this.observed.push(target);
  }
  unobserve() {}
  disconnect() {
    this.disconnected += 1;
  }
}

function stubContext(record: Recorder): CanvasRenderingContext2D {
  const context = {
    globalAlpha: 1,
    lineWidth: 1,
    font: "",
    textAlign: "start",
    textBaseline: "alphabetic",
    lineJoin: "miter",
    setTransform: () => {},
    clearRect: () => {},
    fillRect: () => {
      record.paints += 1;
    },
    beginPath: () => {},
    moveTo: () => {},
    lineTo: () => {},
    arc: () => {},
    stroke: () => {
      record.paints += 1;
    },
    fill: () => {
      record.paints += 1;
    },
    strokeText: () => {
      record.paints += 1;
    },
    fillText: () => {
      record.paints += 1;
    },
  };
  Object.defineProperty(context, "fillStyle", {
    set(value: string) {
      record.fills.push(value);
    },
    get() {
      return record.fills[record.fills.length - 1] ?? "";
    },
  });
  Object.defineProperty(context, "strokeStyle", {
    set(value: string) {
      record.strokes.push(value);
    },
    get() {
      return record.strokes[record.strokes.length - 1] ?? "";
    },
  });
  return context as unknown as CanvasRenderingContext2D;
}

beforeEach(() => {
  recorder = { fills: [], strokes: [], paints: 0, asked: [] };
  observers = [];
  frames = [];
  cancelled = [];
  reducedMotion = false;

  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(
    stubContext(recorder) as unknown as never,
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
      getPropertyValue: (token: string) => {
        recorder.asked.push(token);
        return TOKEN_VALUES[token] ?? "";
      },
    } as unknown as CSSStyleDeclaration;
  }) as typeof window.getComputedStyle);

  vi.stubGlobal("ResizeObserver", StubResizeObserver);
  vi.stubGlobal("requestAnimationFrame", (run: FrameRequestCallback) => {
    const id = frames.length + 1;
    frames.push({ id, run });
    return id;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => {
    cancelled.push(id);
  });
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: query.includes("prefers-reduced-motion") ? reducedMotion : false,
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

function mount() {
  return render(() => (
    <GraphCanvas nodes={NODES} edges={EDGES} focusPath="Alpha.md" onOpen={() => {}} />
  ));
}

describe("the drawing's colours", () => {
  it("come from a token read, never from the file", () => {
    mount();
    const painted = [...recorder.fills, ...recorder.strokes];
    expect(painted.length).toBeGreaterThan(0);
    const fromTokens = new Set(Object.values(TOKEN_VALUES));
    for (const colour of painted) expect(fromTokens.has(colour)).toBe(true);
  });

  it("are every token the drawing says it uses", () => {
    mount();
    for (const token of Object.keys(TOKEN_VALUES)) expect(recorder.asked).toContain(token);
  });

  it("are read again after a theme change lands, not while it is landing", async () => {
    mount();
    expect(recorder.fills).toContain(TOKEN_VALUES["--writ-bg-canvas"]);

    // What the store does: move the signal first, write the root second.
    setThemeChanged(1);
    TOKEN_VALUES["--writ-bg-canvas"] = "rgb(7, 7, 7)";
    await Promise.resolve();

    expect(recorder.fills).toContain("rgb(7, 7, 7)");
    TOKEN_VALUES["--writ-bg-canvas"] = "rgb(1, 1, 1)";
  });

  it("are read again when the theme changes, not once per frame", () => {
    mount();
    const first = recorder.asked.length;
    expect(first).toBeGreaterThanOrEqual(Object.keys(TOKEN_VALUES).length);
    for (const frame of frames.splice(0)) frame.run(0);
    expect(recorder.asked.length).toBe(first);
  });
});

describe("what the drawing tells a reader", () => {
  it("names the note and how many notes are around it", () => {
    const { getByRole } = mount();
    expect(getByRole("img").getAttribute("aria-label")).toBe("Alpha and 2 notes it links with");
  });

  it("says one note, not one notes", () => {
    const { getByRole } = render(() => (
      <GraphCanvas
        nodes={NODES.slice(0, 2)}
        edges={[EDGES[0]]}
        focusPath="Alpha.md"
        onOpen={() => {}}
      />
    ));
    expect(getByRole("img").getAttribute("aria-label")).toBe("Alpha and 1 note it links with");
  });
});

describe("motion", () => {
  it("settles over frames when movement is welcome", () => {
    mount();
    expect(frames.length).toBeGreaterThan(0);
  });

  it("settles at once and paints once when it is not", () => {
    reducedMotion = true;
    mount();
    expect(frames).toEqual([]);
    expect(recorder.paints).toBeGreaterThan(0);
  });
});

describe("what unmounting lets go of", () => {
  it("disconnects the size observer and cancels the frame it asked for", () => {
    const { unmount } = mount();
    expect(observers).toHaveLength(1);
    expect(observers[0].observed).toHaveLength(1);
    const pending = frames[frames.length - 1].id;
    unmount();
    expect(observers[0].disconnected).toBe(1);
    expect(cancelled).toContain(pending);
  });
});
