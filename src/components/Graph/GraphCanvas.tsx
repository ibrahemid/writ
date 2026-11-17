import { createEffect, createMemo, createSignal, on, onCleanup, onMount } from "solid-js";
import { themeStore } from "../../stores/global/theme";
import {
  DEFAULT_LAYOUT_OPTIONS,
  beginLayout,
  nodeAt,
  positions,
  step,
  type LayoutEdge,
  type LayoutOptions,
  type LayoutState,
  type PlacedNode,
} from "../../lib/graph/layout";
import { fitToView, toScreen } from "../../lib/graph/view";
import type { NeighbourhoodNode } from "../../lib/graph/neighbourhood";
import "./GraphCanvas.css";

/** A note to draw. A note handed in with its folder is settled with that
 * folder as its group, which is what a settle that pulls groups together
 * reads. */
export type GraphNode = NeighbourhoodNode & { folder?: string };

interface Props {
  nodes: GraphNode[];
  edges: LayoutEdge[];
  /** The open note, drawn filled while everything around it is drawn hollow. */
  focusPath: string;
  /** Opens a note: a click, or with `onSelect` set a double-click. */
  onOpen: (path: string) => void;
  /**
   * Set when a click should choose a note rather than open it, as the whole
   * folder's drawing does (ADR-037 section 9): a click selects and names it,
   * and a double-click opens it.
   */
  onSelect?: (path: string) => void;
  /** The note a click or a key chose, drawn with a ring of the accent. */
  selectedPath?: string | null;
  /** What the drawing is settled with. The near note's numbers by default. */
  options?: LayoutOptions;
  /** What the drawing is called, for a reader who is never shown it. */
  label?: string;
  /** A colour per note. Without it every note is drawn in the same token. */
  colors?: ReadonlyMap<string, string>;
  /** The notes drawn faint, which is what a search does to the rest. */
  dimmed?: ReadonlySet<string>;
  /**
   * The notes left off the drawing. They keep their place in the settle, so
   * showing them again puts them back where they were; the fit, the pointer
   * and the names skip them.
   */
  hidden?: ReadonlySet<string>;
  /** A second line for the hovered note's name, such as its folder. */
  detailOf?: (path: string) => string;
  /** How far in the drawing is taken, over the size it is fitted at. */
  zoom?: number;
  /** How far the drawing is moved from the middle, in pixels. */
  pan?: { x: number; y: number };
  /** Set to let a pointer drag move the drawing. */
  onPanBy?: (dx: number, dy: number) => void;
  /** Set to let a pinch, or the wheel with Ctrl or Cmd held, take the
   * drawing in and out. A plain scroll moves it (`onPanBy`). */
  onZoomBy?: (factor: number) => void;
  /** Puts the canvas in the tab order, for a drawing that is moved by keys. */
  focusable?: boolean;
  /**
   * Set when the drawing is of a set of notes rather than of one note.
   *
   * A drawing of one note is a new drawing per note, settled from that note's
   * own seed. A folder is one drawing that notes are written into and deleted
   * from: it settles from a seed of its own, so which note is open says
   * nothing about its shape, and a note that is still there stays where it
   * settled when the set changes around it.
   */
  keepsPlaces?: boolean;
  class?: string;
}

/** The room a drawing is fitted into before the canvas has been measured. */
const DEFAULT_SIZE = { width: 216, height: 160 };

/** How far the drawing stays from the canvas edge, in CSS pixels. */
const VIEW_PADDING = 14;

/** The size every name is drawn at is the canvas's own, held inside this range. */
const LABEL_MIN_SIZE = 10;
const LABEL_MAX_SIZE = 12;

/** How tall a line of names is, over the size it is drawn at. */
const LABEL_LINE = 1.3;

/** How many of the most linked notes are named at the fitted size. The count
 * grows with the area the drawing is taken in to. */
const LABEL_BUDGET = 24;

/** The longest name drawn whole; past it the name is cut and ends in "…". */
const LABEL_MAX_CHARS = 28;
const LABEL_MAX_CHARS_PRIMARY = 48;

/** How far a name sits under its disc. */
const LABEL_GAP = 3;

/** The side of one cell of the grid that keeps two names from overlapping. */
const LABEL_CELL = 6;

/** What a glyph is taken to be wide, over the size, where text cannot be measured. */
const GLYPH_WIDTH = 0.58;

/** How many measured names are kept before the measures are taken again. */
const WIDTHS_KEPT = 8192;

/** The most of a settle that runs per frame, in steps and in milliseconds. */
const STEPS_PER_FRAME = 8;
const FRAME_BUDGET_MS = 8;

/**
 * How long a settle nobody watches runs for before it hands the frame back,
 * and how many notes are settled in one go rather than in pieces.
 *
 * Reduced motion settles at once and paints the answer, which is a fraction of
 * a millisecond for the notes around one note. A whole folder is seconds of
 * arithmetic, and a window that stops answering for seconds is worse than the
 * movement the setting asked to be spared: past this many notes the same
 * settle runs in pieces across frames and still paints once, at the end.
 */
const SETTLE_BUDGET_MS = 12;
const SETTLE_AT_ONCE = 400;

/**
 * A disc grows with the square root of its links, from the smallest a linked
 * note is drawn at to the largest any is, in CSS pixels. A note nothing links
 * with is drawn smaller than either, and fainter.
 */
const RADIUS_MIN = 3;
const RADIUS_PER_ROOT_LINK = 1.2;
const RADIUS_MAX = 9;
const RADIUS_UNLINKED = 2.25;
const UNLINKED_ALPHA = 0.5;

/** The open note is never drawn smaller than this. */
const RADIUS_FOCUS = 6;

/**
 * How faint a link is drawn against the notes it joins. Past `EDGE_CROWD`
 * links the alpha falls with the square root of the count, down to
 * `EDGE_ALPHA_MIN`, so a large folder reads as clusters rather than a mesh.
 */
const EDGE_ALPHA = 0.4;
const EDGE_ALPHA_MIN = 0.2;
const EDGE_CROWD = 400;

/** While a note is hovered or chosen: its links, and every other link. */
const EDGE_LIT_WIDTH = 1.6;
const EDGE_UNLIT_SHARE = 0.3;

/** How far a pointer may travel before the release counts as a drag. */
const DRAG_SLOP = 3;

/** How much one notch of the wheel takes the drawing in or out. */
const WHEEL_ZOOM = 0.0015;

/** How far a wheel line is, for a wheel that counts in lines. */
const WHEEL_LINE = 16;

/** How far outside a disc the ring of the chosen note is drawn. */
const SELECTED_RING_GAP = 3;

/** How far the hovered name sits from the pointer, and how near the top the
 * pointer may be before the name moves under it. */
const HOVER_OFFSET = 12;
const HOVER_FLIP = 56;

/** How much ground is laid back around the name where a link runs under it. */
const LABEL_HALO = 3;

/** The names the drawing is painted with. Nothing here is a colour. */
const TOKENS = {
  ground: "--writ-bg-canvas",
  edge: "--writ-fg-muted",
  muted: "--writ-fg-muted",
  label: "--writ-fg",
  focus: "--writ-accent",
  ring: "--writ-border",
  nodeFill: "--writ-bg-raised",
  faint: "--writ-fg-faint",
} as const;

type Palette = Record<keyof typeof TOKENS, string> & {
  /** The canvas element's own face and size, so the label follows the app's. */
  face: string;
  fontSize: number;
  /** How faint the app draws a thing it is not pointing at. */
  dimmed: number;
};

/**
 * A seed from the note's own path, so one note settles into the same shape
 * every time it is opened rather than into a new one per visit.
 */
function seedFor(path: string): number {
  let seed = 0x811c9dc5 | 0;
  for (let i = 0; i < path.length; i += 1) {
    seed = (Math.imul(seed ^ path.charCodeAt(i), 16777619) + 1) | 0;
  }
  return seed;
}

/** The seed a drawing that is of no one note settles from. */
const OWN_SEED = seedFor("");

function samePalette(a: Palette, b: Palette): boolean {
  return (Object.keys(a) as (keyof Palette)[]).every((key) => a[key] === b[key]);
}

function radiusFor(degree: number): number {
  if (degree <= 0) return RADIUS_UNLINKED;
  const wanted = RADIUS_MIN + (Math.sqrt(degree) - 1) * RADIUS_PER_ROOT_LINK;
  return wanted > RADIUS_MAX ? RADIUS_MAX : wanted;
}

function edgeAlphaFor(count: number): number {
  if (count <= EDGE_CROWD) return EDGE_ALPHA;
  const wanted = EDGE_ALPHA * Math.sqrt(EDGE_CROWD / count);
  return wanted < EDGE_ALPHA_MIN ? EDGE_ALPHA_MIN : wanted;
}

function clampLabelSize(size: number): number {
  if (size < LABEL_MIN_SIZE) return LABEL_MIN_SIZE;
  return size > LABEL_MAX_SIZE ? LABEL_MAX_SIZE : size;
}

function trimName(name: string, most: number): string {
  return name.length > most ? `${name.slice(0, most - 1)}…` : name;
}

interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * Which parts of the canvas a name already covers, in cells `LABEL_CELL`
 * across. A name is placed only where every cell under it is free, which is
 * a few dozen reads a name rather than a walk over every name placed before.
 */
class Occupancy {
  private cells = new Uint8Array(0);
  private columns = 0;
  private rows = 0;

  reset(width: number, height: number): void {
    const columns = Math.max(1, Math.ceil(width / LABEL_CELL));
    const rows = Math.max(1, Math.ceil(height / LABEL_CELL));
    if (columns * rows !== this.cells.length) this.cells = new Uint8Array(columns * rows);
    else this.cells.fill(0);
    this.columns = columns;
    this.rows = rows;
  }

  /** The cell span under a box, or `null` for a box wholly off the canvas. */
  private span(box: Box): [number, number, number, number] | null {
    const c0 = Math.max(0, Math.floor(box.left / LABEL_CELL));
    const c1 = Math.min(this.columns - 1, Math.floor(box.right / LABEL_CELL));
    const r0 = Math.max(0, Math.floor(box.top / LABEL_CELL));
    const r1 = Math.min(this.rows - 1, Math.floor(box.bottom / LABEL_CELL));
    return c0 > c1 || r0 > r1 ? null : [c0, c1, r0, r1];
  }

  isFree(box: Box): boolean {
    const span = this.span(box);
    if (!span) return false;
    const [c0, c1, r0, r1] = span;
    for (let row = r0; row <= r1; row += 1) {
      for (let column = c0; column <= c1; column += 1) {
        if (this.cells[row * this.columns + column] !== 0) return false;
      }
    }
    return true;
  }

  take(box: Box): void {
    const span = this.span(box);
    if (!span) return;
    const [c0, c1, r0, r1] = span;
    for (let row = r0; row <= r1; row += 1) {
      this.cells.fill(1, row * this.columns + c0, row * this.columns + c1 + 1);
    }
  }
}

/** How a placed name is drawn: plainly, as a neighbour of what is lit, or as
 * the hovered, chosen or open note. */
type LabelTier = "ranked" | "near" | "primary";

interface PlacedLabel {
  text: string;
  x: number;
  top: number;
  tier: LabelTier;
}

function now(): number {
  return typeof performance === "undefined" ? Date.now() : performance.now();
}

/**
 * The notes around the open one, drawn.
 *
 * The open note is the one filled disc and the only thing wearing the accent;
 * its neighbours are hollow, and a link is a hairline quiet enough that a
 * dense note still reads as a shape rather than a mesh. The open, hovered and
 * chosen notes are always named, and the most linked of the rest are named
 * wherever a name fits without covering another (`drawLabels`). Every note it
 * draws is listed as text above it either way, under "Links from this note" or
 * under "Links to this note".
 *
 * The same canvas draws a whole folder (`FolderGraphView`), which is the same
 * drawing with a colour per folder, a search that dims what it does not name,
 * and a drawing that can be moved and taken in and out. What that view adds is
 * passed in rather than forked: one canvas, one settle, one way of reading it.
 */
export default function GraphCanvas(props: Props) {
  let canvas: HTMLCanvasElement | undefined;
  let context: CanvasRenderingContext2D | null = null;
  let frame: number | null = null;
  let observer: ResizeObserver | null = null;
  let state: LayoutState | null = null;
  let palette: Palette | null = null;
  let placed: PlacedNode[] = [];
  let live = true;
  let size = { ...DEFAULT_SIZE };
  const occupancy = new Occupancy();
  /** How wide a name is at a font, measured once. */
  const widths = new Map<string, number>();
  /** Where a press started, and whether it has travelled far enough to drag. */
  let press: { x: number; y: number; dragging: boolean } | null = null;

  const [hovered, setHovered] = createSignal<string | null>(null);
  /** Where the pointer is over the drawing, which the hovered name follows. */
  const [pointer, setPointer] = createSignal<{ x: number; y: number }>({ x: 0, y: 0 });

  const options = () => props.options ?? DEFAULT_LAYOUT_OPTIONS;
  /**
   * What the drawing is of: the notes and the links between them, in an order
   * two identical sets agree on.
   *
   * A drawing that keeps its places settles on this and on nothing else. What
   * was searched for, where the drawing has been taken and which note is open
   * all paint the same settle again, so choosing a note from a folder of two
   * thousand does not rearrange the two thousand under the choice.
   */
  const drawnSet = createMemo(() => {
    const paths = props.nodes.map((node) => node.path).sort();
    const links = props.edges
      .map((edge) => (edge.from < edge.to ? [edge.from, edge.to] : [edge.to, edge.from]))
      .map(([from, to]) => `${from}\u0000${to}`)
      .sort();
    return JSON.stringify([paths, links]);
  });
  const focusNode = () => props.nodes.find((node) => node.path === props.focusPath) ?? null;
  const neighbourCount = () => Math.max(0, props.nodes.length - 1);

  /** The notes by how many links they carry, most first, ties by path: the
   * order the names beyond the lit ones are handed out in. */
  const ranked = createMemo(() =>
    [...props.nodes].sort(
      (a, b) => b.degree - a.degree || (a.path < b.path ? -1 : a.path > b.path ? 1 : 0),
    ),
  );

  /** Every note's neighbours, both directions. */
  const adjacency = createMemo(() => {
    const near = new Map<string, string[]>();
    const add = (from: string, to: string) => {
      const list = near.get(from);
      if (list) list.push(to);
      else near.set(from, [to]);
    };
    for (const edge of props.edges) {
      if (edge.from === edge.to) continue;
      add(edge.from, edge.to);
      add(edge.to, edge.from);
    }
    return near;
  });

  const names = createMemo(
    () => new Map(props.nodes.map((node) => [node.path, node.name] as const)),
  );

  const hoveredNode = createMemo(() => {
    const path = hovered();
    return path === null ? null : (props.nodes.find((node) => node.path === path) ?? null);
  });

  /** The hovered note's second line, kept off the text so the name alone is
   * what the line reads as. */
  const hoveredDetail = () => {
    const node = hoveredNode();
    if (!node || !props.detailOf) return undefined;
    return props.detailOf(node.path) || undefined;
  };

  /** A layout node per drawn note, with its folder as its group. */
  const layoutNodes = () =>
    props.nodes.map((node) =>
      node.folder === undefined ? { path: node.path } : { path: node.path, group: node.folder },
    );

  /** What the drawing is, for a reader who is never shown the drawing. */
  const description = () => {
    if (props.label) return props.label;
    const name = focusNode()?.name ?? "";
    const count = neighbourCount();
    if (count === 1) return `${name} and 1 file it links with`;
    return `${name} and ${count} files it links with`;
  };

  function readPalette(element: HTMLCanvasElement): Palette {
    const style = getComputedStyle(element);
    const read = (token: string) => style.getPropertyValue(token).trim();
    return {
      ground: read(TOKENS.ground),
      edge: read(TOKENS.edge),
      muted: read(TOKENS.muted),
      label: read(TOKENS.label),
      focus: read(TOKENS.focus),
      ring: read(TOKENS.ring),
      nodeFill: read(TOKENS.nodeFill),
      faint: read(TOKENS.faint),
      face: style.fontFamily,
      fontSize: Number.parseFloat(style.fontSize) || LABEL_MIN_SIZE,
      dimmed: Number.parseFloat(read("--writ-icon-opacity")) || 1,
    };
  }

  /** How wide `text` is in the context's current font, measured once. */
  function widthOf(ctx: CanvasRenderingContext2D, text: string, fontSize: number): number {
    const key = `${ctx.font}\u0000${text}`;
    const known = widths.get(key);
    if (known !== undefined) return known;
    const width =
      typeof ctx.measureText === "function"
        ? ctx.measureText(text).width
        : text.length * fontSize * GLYPH_WIDTH;
    if (widths.size > WIDTHS_KEPT) widths.clear();
    widths.set(key, width);
    return width;
  }

  /**
   * The links, as one faint stroke, or with a note hovered or chosen as two:
   * every link that touches it at full strength and a little wider, and the
   * rest fainter than usual. Each link is drawn once either way.
   */
  function drawEdges(
    ctx: CanvasRenderingContext2D,
    paint: Palette,
    by: ReadonlyMap<string, PlacedNode>,
    lit: ReadonlySet<string>,
  ): void {
    const dim = props.dimmed;
    const base = edgeAlphaFor(props.edges.length);
    const touching: [PlacedNode, PlacedNode][] = [];
    ctx.globalAlpha = lit.size > 0 ? base * EDGE_UNLIT_SHARE : base;
    ctx.strokeStyle = paint.edge;
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (const edge of props.edges) {
      const from = by.get(edge.from);
      const to = by.get(edge.to);
      if (!from || !to) continue;
      // A link between two notes a search did not name says nothing about
      // what was searched for, and at a folder's size those links are most of
      // them: leaving them out is what turns the drawing back into a shape.
      if (dim && dim.has(edge.from) && dim.has(edge.to)) continue;
      if (lit.has(edge.from) || lit.has(edge.to)) {
        touching.push([from, to]);
        continue;
      }
      ctx.moveTo(from.x, from.y);
      ctx.lineTo(to.x, to.y);
    }
    ctx.stroke();
    if (touching.length > 0) {
      ctx.globalAlpha = 1;
      ctx.lineWidth = EDGE_LIT_WIDTH;
      ctx.beginPath();
      for (const [from, to] of touching) {
        ctx.moveTo(from.x, from.y);
        ctx.lineTo(to.x, to.y);
      }
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    ctx.lineWidth = 1;
  }

  /**
   * The names on the drawing.
   *
   * The hovered, chosen and open notes are always named, in the strong face,
   * and so are the neighbours of what is hovered or chosen. Then the most
   * linked notes on screen, as many as the budget for this zoom allows, each
   * placed only where no name already is, which the occupancy grid answers
   * per name; a name that would overlap is skipped, not shrunk. Every
   * name is drawn at the canvas's size held between ten and twelve pixels,
   * whatever the fit, so none is drawn too small to read.
   */
  function drawLabels(
    ctx: CanvasRenderingContext2D,
    paint: Palette,
    by: ReadonlyMap<string, PlacedNode>,
    lit: ReadonlySet<string>,
    primary: readonly (string | null)[],
    zoom: number,
  ): void {
    const fontSize = clampLabelSize(paint.fontSize);
    const line = fontSize * LABEL_LINE;
    const regular = `${fontSize}px ${paint.face}`;
    const strong = `600 ${fontSize}px ${paint.face}`;
    const dim = props.dimmed;
    const nameOf = names();
    const labels: PlacedLabel[] = [];
    const named = new Set<string>();
    occupancy.reset(size.width, size.height);

    const place = (path: string, tier: LabelTier, force: boolean): boolean => {
      if (named.has(path)) return false;
      const node = by.get(path);
      const name = nameOf.get(path);
      if (!node || !name) return false;
      // A note taken off screen keeps its name off screen too, rather than
      // pinned to an edge away from the disc it names.
      if (node.x < 0 || node.x > size.width || node.y < 0 || node.y > size.height) return false;
      const most = tier === "primary" ? LABEL_MAX_CHARS_PRIMARY : LABEL_MAX_CHARS;
      const text = trimName(name, most);
      const width = widthOf(ctx, text, fontSize);
      let left = node.x - width / 2;
      if (left + width > size.width - LABEL_HALO) left = size.width - LABEL_HALO - width;
      if (left < LABEL_HALO) left = LABEL_HALO;
      let top = node.y + node.radius + LABEL_GAP;
      if (top + line > size.height) top = node.y - node.radius - LABEL_GAP - line;
      const box = {
        left: left - LABEL_HALO,
        top,
        right: left + width + LABEL_HALO,
        bottom: top + line,
      };
      if (!force && !occupancy.isFree(box)) return false;
      occupancy.take(box);
      named.add(path);
      labels.push({ text, x: left + width / 2, top, tier });
      return true;
    };

    ctx.font = strong;
    for (const path of primary) if (path) place(path, "primary", true);

    ctx.font = regular;
    const order = ranked();
    if (lit.size > 0) {
      const near = new Set<string>();
      const around = adjacency();
      for (const path of lit) for (const other of around.get(path) ?? []) near.add(other);
      for (const node of order) if (near.has(node.path)) place(node.path, "near", false);
    }

    let budget = Math.round(LABEL_BUDGET * zoom * zoom);
    for (const node of order) {
      if (budget <= 0) break;
      if (named.has(node.path) || (dim && dim.has(node.path))) continue;
      if (place(node.path, "ranked", false)) budget -= 1;
    }

    if (labels.length === 0) return;
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    ctx.lineJoin = "round";
    ctx.lineWidth = LABEL_HALO;
    ctx.strokeStyle = paint.ground;
    // Drawn plainest first, so a name that is always drawn sits over any it
    // was allowed to cover.
    for (const tier of ["ranked", "near", "primary"] as const) {
      let ready = false;
      for (const label of labels) {
        if (label.tier !== tier) continue;
        if (!ready) {
          ctx.font = tier === "primary" ? strong : regular;
          ctx.fillStyle = tier === "ranked" ? paint.muted : paint.label;
          ready = true;
        }
        // The name lands wherever the note settled, which is sometimes over
        // a link. Laying the ground back down around the letters keeps it
        // legible without a plate to draw or a second colour to pick.
        ctx.strokeText(label.text, label.x, label.top);
        ctx.fillText(label.text, label.x, label.top);
      }
    }
  }

  function draw() {
    const element = canvas;
    const ctx = context;
    if (!element || !ctx || !palette || !state) return;

    const paint = palette;
    const points = positions(state);
    const hidden = props.hidden;
    const shown =
      hidden && hidden.size > 0 ? props.nodes.filter((node) => !hidden.has(node.path)) : props.nodes;
    // The settle works in world units and says nothing about how big the panel
    // is; what fits it into this canvas is the one scale and shift below, so
    // the drawing keeps its proportions at any note count and at any width.
    // Every note is fitted with the room the largest disc takes rather than
    // the room its own takes, so the fit is a property of where the notes
    // settled and of nothing else. The open note is the one disc drawn at that
    // size, and opening one from the drawing would otherwise re-fit every
    // other note around it.
    const world: PlacedNode[] = shown.map((node) => ({
      path: node.path,
      ...(points.get(node.path) ?? { x: 0, y: 0 }),
      radius: RADIUS_MAX,
    }));
    const fitted = fitToView(world, {
      width: size.width,
      height: size.height,
      padding: VIEW_PADDING,
    });
    // Taking the drawing in and out turns the fit, and moving it shifts what
    // the fit centred: the middle of the canvas stays the middle whatever the
    // zoom, so taking it in does not walk the drawing off the edge.
    const zoom = props.zoom ?? 1;
    const pan = props.pan ?? { x: 0, y: 0 };
    const view = {
      scale: fitted.scale * zoom,
      offsetX: (fitted.offsetX - size.width / 2) * zoom + size.width / 2 + pan.x,
      offsetY: (fitted.offsetY - size.height / 2) * zoom + size.height / 2 + pan.y,
    };
    placed = world.map((node, at) => {
      const own = radiusFor(shown[at].degree);
      return {
        ...node,
        radius: node.path === props.focusPath && own < RADIUS_FOCUS ? RADIUS_FOCUS : own,
        ...toScreen(node, view),
      };
    });
    const by = new Map(placed.map((node) => [node.path, node] as const));

    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, element.width, element.height);
    const ratio = element.width / size.width || 1;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);

    ctx.fillStyle = paint.ground;
    ctx.fillRect(0, 0, size.width, size.height);

    const under = hovered();
    const chosenPath = props.selectedPath ?? null;
    const lit = new Set<string>();
    if (under !== null && by.has(under)) lit.add(under);
    if (chosenPath !== null && by.has(chosenPath)) lit.add(chosenPath);
    drawEdges(ctx, paint, by, lit);

    const dim = props.dimmed;
    const color = props.colors;
    for (let at = 0; at < placed.length; at += 1) {
      const node = placed[at];
      const isFocus = node.path === props.focusPath;
      const faded = dim ? dim.has(node.path) : false;
      // A note nothing links with is a fact worth seeing and not one worth
      // the eye: drawn small and faint, unless it is the one being looked at.
      const quiet = !faded && !isFocus && !lit.has(node.path) && shown[at].degree === 0;
      ctx.globalAlpha = faded ? paint.dimmed : quiet ? UNLINKED_ALPHA : 1;
      ctx.beginPath();
      ctx.arc(node.x, node.y, node.radius, 0, Math.PI * 2);
      if (color) {
        // A folder's colour is what says which folder a note is in, so it is
        // what the disc is filled with; the open note is named by a ring
        // rather than by taking the accent off its own folder.
        ctx.fillStyle = faded ? paint.faint : (color.get(node.path) ?? paint.nodeFill);
        ctx.fill();
        if (isFocus) {
          ctx.lineWidth = 2;
          ctx.strokeStyle = paint.label;
          ctx.stroke();
        } else if (node.path === under) {
          ctx.lineWidth = 1.5;
          ctx.strokeStyle = paint.label;
          ctx.stroke();
        }
      } else {
        ctx.fillStyle = isFocus ? paint.focus : paint.nodeFill;
        ctx.fill();
        if (!isFocus) {
          ctx.lineWidth = node.path === under ? 1.5 : 1;
          ctx.strokeStyle = node.path === under ? paint.edge : paint.ring;
          ctx.stroke();
        }
      }
    }
    ctx.globalAlpha = 1;

    const chosen = chosenPath ? by.get(chosenPath) : undefined;
    if (chosen) {
      ctx.beginPath();
      ctx.arc(chosen.x, chosen.y, chosen.radius + SELECTED_RING_GAP, 0, Math.PI * 2);
      ctx.lineWidth = 2;
      ctx.strokeStyle = paint.focus;
      ctx.stroke();
    }

    drawLabels(ctx, paint, by, lit, [under, chosenPath, props.focusPath], zoom);
  }

  function cancelFrame() {
    if (frame !== null) {
      cancelAnimationFrame(frame);
      frame = null;
    }
  }

  /** Whether the person asked for as little movement as the app can manage. */
  function settlesAtOnce(): boolean {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return true;
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  /**
   * Runs the settle for as long as one frame can spare.
   *
   * A near note's whole settle costs less than a millisecond, so the step
   * count is what paces it and the drawing opens out over half a second. A
   * folder of two thousand notes costs milliseconds a step, and there the
   * clock is what stops the frame: one step lands, the drawing is painted, and
   * the window stays answerable throughout.
   */
  function runSteps(budget: number, cap: number): void {
    if (!state) return;
    const until = now() + budget;
    let taken = 0;
    while (!state.done && taken < cap) {
      state = step(state);
      taken += 1;
      if (now() >= until) return;
    }
  }

  function runFrame() {
    frame = null;
    if (!state) return;
    runSteps(FRAME_BUDGET_MS, STEPS_PER_FRAME);
    draw();
    if (!state.done) frame = requestAnimationFrame(runFrame);
  }

  /** The settle nobody watches: no paint until it is finished. */
  function runQuietly() {
    frame = null;
    if (!state) return;
    runSteps(SETTLE_BUDGET_MS, Number.POSITIVE_INFINITY);
    if (state.done) {
      draw();
      return;
    }
    frame = requestAnimationFrame(runQuietly);
  }

  /** Runs the whole settle here and now, however long it takes. */
  function runToEnd() {
    while (state && !state.done) state = step(state);
  }

  /**
   * Starts the settle again, which is what a new note, a new size or a first
   * paint all are. Reduced motion runs the whole settle here and paints the
   * answer once: no frame is asked for, so there is nothing to see moving.
   */
  function restart() {
    cancelFrame();
    if (props.nodes.length === 0) return;
    const warm = props.keepsPlaces && state ? positions(state) : undefined;
    const seed = props.keepsPlaces ? OWN_SEED : seedFor(props.focusPath);
    state = beginLayout(layoutNodes(), props.edges, options(), seed, warm);
    if (settlesAtOnce()) {
      if (props.nodes.length <= SETTLE_AT_ONCE) {
        runToEnd();
        draw();
        return;
      }
      frame = requestAnimationFrame(runQuietly);
      return;
    }
    // The first frame is painted here rather than waited for, so the drawing
    // arrives with the section and then opens out.
    draw();
    frame = requestAnimationFrame(runFrame);
  }

  function measure(element: HTMLCanvasElement) {
    const ratio = typeof window === "undefined" ? 1 : window.devicePixelRatio || 1;
    const rect = element.getBoundingClientRect();
    const width = rect.width || DEFAULT_SIZE.width;
    const height = rect.height || DEFAULT_SIZE.height;
    const changed = width !== size.width || height !== size.height;
    size = { width, height };
    element.width = Math.round(width * ratio);
    element.height = Math.round(height * ratio);
    return changed;
  }

  function pointIn(element: HTMLCanvasElement, event: PointerEvent | MouseEvent) {
    const rect = element.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  }

  onMount(() => {
    const element = canvas;
    if (!element) return;
    context = element.getContext("2d");
    palette = readPalette(element);
    measure(element);
    restart();

    if (typeof ResizeObserver !== "undefined") {
      observer = new ResizeObserver(() => {
        if (measure(element)) restart();
        else draw();
      });
      observer.observe(element);
    }
  });

  /**
   * Reads the palette again when the theme's tokens change, and only then, so
   * a frame costs no style resolution.
   *
   * The read waits a microtask because the store writes its signal before it
   * writes the custom properties to the root: an effect that reads the canvas
   * the moment the signal changes reads the palette the theme is leaving. A
   * microtask lands after the whole change, and asks for no frame, so a
   * repaint here is the same repaint under reduced motion.
   */
  createEffect(() => {
    themeStore.resolvedTokens();
    queueMicrotask(() => {
      if (!live || !canvas) return;
      const next = readPalette(canvas);
      if (palette && samePalette(palette, next)) return;
      palette = next;
      draw();
    });
  });

  // A drawing of one note settles from that note, so the note it is under is
  // part of what it is a drawing of: two notes that link to each other give the
  // section the same pair either way round, and it is still another drawing.
  // A drawing that keeps its places settles from a seed of its own, so the note
  // it rings says nothing about its shape and only the set restarts it.
  const settleKey = createMemo(() =>
    props.keepsPlaces ? drawnSet() : `${props.focusPath ?? ""}\n${drawnSet()}`,
  );

  createEffect(
    on(
      () => [settleKey(), props.options] as const,
      () => {
        if (canvas) restart();
      },
      { defer: true },
    ),
  );

  // Moving the drawing, taking it in and out, searching it, leaving notes off
  // it, and in a drawing that keeps its places opening a note from it, all
  // change what is painted and none of them change where a note settled, so
  // they repaint rather than start the settle again.
  createEffect(
    on(
      () =>
        [
          props.zoom,
          props.pan,
          props.dimmed,
          props.hidden,
          props.colors,
          props.focusPath,
          props.selectedPath,
        ] as const,
      () => {
        if (canvas && state) draw();
      },
      { defer: true },
    ),
  );

  onCleanup(() => {
    live = false;
    cancelFrame();
    observer?.disconnect();
    observer = null;
  });

  return (
    <div class={`graph ${props.class ?? ""}`.trim()}>
      <canvas
        class="graph-canvas"
        classList={{ "is-over": hovered() !== null }}
        role={props.focusable ? "application" : "img"}
        aria-roledescription={props.focusable ? "graph" : undefined}
        aria-label={description()}
        tabindex={props.focusable ? 0 : undefined}
        ref={canvas}
        onPointerDown={(event) => {
          if (!props.onPanBy) return;
          press = { x: event.clientX, y: event.clientY, dragging: false };
          canvas?.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => {
          const element = canvas;
          if (!element) return;
          if (press && props.onPanBy) {
            const dx = event.clientX - press.x;
            const dy = event.clientY - press.y;
            if (press.dragging || Math.abs(dx) > DRAG_SLOP || Math.abs(dy) > DRAG_SLOP) {
              press = { x: event.clientX, y: event.clientY, dragging: true };
              props.onPanBy(dx, dy);
              return;
            }
          }
          const at = pointIn(element, event);
          setPointer(at);
          const next = nodeAt(placed, at);
          if (next !== hovered()) {
            setHovered(next);
            draw();
          }
        }}
        onPointerUp={(event) => {
          if (press) canvas?.releasePointerCapture(event.pointerId);
        }}
        onPointerLeave={() => {
          if (hovered() !== null) {
            setHovered(null);
            draw();
          }
        }}
        onWheel={(event) => {
          // A trackpad pinch arrives as the wheel with Ctrl held, which is
          // also how a mouse asks to zoom; anything else is a scroll, and a
          // scroll moves the drawing the way it moves a page.
          const zooms = event.ctrlKey || event.metaKey;
          if (zooms ? !props.onZoomBy : !props.onPanBy) return;
          event.preventDefault();
          const unit = event.deltaMode === 1 ? WHEEL_LINE : 1;
          if (zooms) props.onZoomBy!(Math.exp(-event.deltaY * unit * WHEEL_ZOOM));
          else props.onPanBy!(-event.deltaX * unit, -event.deltaY * unit);
        }}
        onClick={(event) => {
          const element = canvas;
          if (!element) return;
          // A drag that ends over a note is a drag, not a click on the note.
          const dragged = press?.dragging ?? false;
          press = null;
          if (dragged) return;
          const picked = nodeAt(placed, pointIn(element, event));
          if (picked === null) return;
          if (props.onSelect) props.onSelect(picked);
          else props.onOpen(picked);
        }}
        onDblClick={(event) => {
          const element = canvas;
          if (!element || !props.onSelect) return;
          const picked = nodeAt(placed, pointIn(element, event));
          if (picked !== null) props.onOpen(picked);
        }}
      />
      <p
        class="graph-hovered"
        classList={{
          "is-left": pointer().x > size.width / 2,
          "is-below": pointer().y < HOVER_FLIP,
        }}
        data-detail={hoveredDetail()}
        style={{
          left: `${pointer().x + (canvas?.offsetLeft ?? 0)}px`,
          top: `${
            pointer().y +
            (canvas?.offsetTop ?? 0) +
            (pointer().y < HOVER_FLIP ? HOVER_OFFSET : -HOVER_OFFSET)
          }px`,
        }}
      >
        {hoveredNode()?.name ?? ""}
      </p>
    </div>
  );
}
