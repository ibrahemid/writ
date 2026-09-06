import { createEffect, createSignal, on, onCleanup, onMount } from "solid-js";
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

interface Props {
  nodes: NeighbourhoodNode[];
  edges: LayoutEdge[];
  /** The open note, drawn filled while everything around it is drawn hollow. */
  focusPath: string;
  onOpen: (path: string) => void;
  /** What the drawing is settled with. The near note's numbers by default. */
  options?: LayoutOptions;
  /** What the drawing is called, for a reader who is never shown it. */
  label?: string;
  /** A colour per note. Without it every note is drawn in the same token. */
  colors?: ReadonlyMap<string, string>;
  /** The notes drawn faint, which is what a search does to the rest. */
  dimmed?: ReadonlySet<string>;
  /** How far in the drawing is taken, over the size it is fitted at. */
  zoom?: number;
  /** How far the drawing is moved from the middle, in pixels. */
  pan?: { x: number; y: number };
  /** Set to let a pointer drag move the drawing. */
  onPanBy?: (dx: number, dy: number) => void;
  /** Set to let the wheel take the drawing in and out. */
  onZoomBy?: (factor: number) => void;
  /** Puts the canvas in the tab order, for a drawing that is moved by keys. */
  focusable?: boolean;
  class?: string;
}

/** The room a drawing is fitted into before the canvas has been measured. */
const DEFAULT_SIZE = { width: 216, height: 160 };

/** How far the drawing stays from the canvas edge, in CSS pixels. */
const VIEW_PADDING = 14;

/** How small the open note's name may be drawn, and when it is left off. */
const LABEL_MIN_SIZE = 9;

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

/** The smallest disc, and how much each extra link adds, in CSS pixels. */
const RADIUS_BASE = 3.5;
const RADIUS_PER_LINK = 0.9;
const RADIUS_MAX = 7;

/** How faint a link is drawn against the notes it joins. */
const EDGE_ALPHA = 0.4;

/** How far a pointer may travel before the release counts as a drag. */
const DRAG_SLOP = 3;

/** How much one notch of the wheel takes the drawing in or out. */
const WHEEL_ZOOM = 0.0015;

/** How far the open note's name sits under its disc. */
const LABEL_OFFSET = 12;

/** How much ground is laid back around the name where a link runs under it. */
const LABEL_HALO = 3;

/** The names the drawing is painted with. Nothing here is a colour. */
const TOKENS = {
  ground: "--writ-bg-canvas",
  edge: "--writ-fg-muted",
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

function samePalette(a: Palette, b: Palette): boolean {
  return (Object.keys(a) as (keyof Palette)[]).every((key) => a[key] === b[key]);
}

function radiusFor(degree: number): number {
  const wanted = RADIUS_BASE + degree * RADIUS_PER_LINK;
  return wanted > RADIUS_MAX ? RADIUS_MAX : wanted;
}

function now(): number {
  return typeof performance === "undefined" ? Date.now() : performance.now();
}

/**
 * The notes around the open one, drawn.
 *
 * The open note is the one filled disc and the only thing wearing the accent;
 * its neighbours are hollow, and a link is a hairline quiet enough that a
 * dense note still reads as a shape rather than a mesh. Only the open note's
 * name is painted on: at this width the rest would collide, so the drawing
 * says where a note sits and hovering one says which it is. Every note it
 * draws is listed as text above it either way, under "Links" or under "Links
 * to this note".
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
  /** Where a press started, and whether it has travelled far enough to drag. */
  let press: { x: number; y: number; dragging: boolean } | null = null;

  const [hovered, setHovered] = createSignal<string | null>(null);

  const options = () => props.options ?? DEFAULT_LAYOUT_OPTIONS;
  const focusNode = () => props.nodes.find((node) => node.path === props.focusPath) ?? null;
  const neighbourCount = () => Math.max(0, props.nodes.length - 1);

  /** What the drawing is, for a reader who is never shown the drawing. */
  const description = () => {
    if (props.label) return props.label;
    const name = focusNode()?.name ?? "";
    const count = neighbourCount();
    if (count === 1) return `${name} and 1 note it links with`;
    return `${name} and ${count} notes it links with`;
  };

  function readPalette(element: HTMLCanvasElement): Palette {
    const style = getComputedStyle(element);
    const read = (token: string) => style.getPropertyValue(token).trim();
    return {
      ground: read(TOKENS.ground),
      edge: read(TOKENS.edge),
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

  function draw() {
    const element = canvas;
    const ctx = context;
    if (!element || !ctx || !palette || !state) return;

    const paint = palette;
    const points = positions(state);
    // The settle works in world units and says nothing about how big the panel
    // is; what fits it into this canvas is the one scale and shift below, so
    // the drawing keeps its proportions at any note count and at any width.
    const world: PlacedNode[] = props.nodes.map((node) => ({
      path: node.path,
      ...(points.get(node.path) ?? { x: 0, y: 0 }),
      radius: node.path === props.focusPath ? RADIUS_MAX : radiusFor(node.degree),
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
    placed = world.map((node) => ({ ...node, ...toScreen(node, view) }));
    const by = new Map(placed.map((node) => [node.path, node] as const));

    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, element.width, element.height);
    const ratio = element.width / size.width || 1;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);

    ctx.fillStyle = paint.ground;
    ctx.fillRect(0, 0, size.width, size.height);

    const dim = props.dimmed;
    ctx.globalAlpha = EDGE_ALPHA;
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
      ctx.moveTo(from.x, from.y);
      ctx.lineTo(to.x, to.y);
    }
    ctx.stroke();
    ctx.globalAlpha = 1;

    const under = hovered();
    const color = props.colors;
    for (const node of placed) {
      const isFocus = node.path === props.focusPath;
      const faded = dim ? dim.has(node.path) : false;
      ctx.globalAlpha = faded ? paint.dimmed : 1;
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

    const focus = by.get(props.focusPath);
    const name = focusNode()?.name;
    // A crowded folder is drawn small, and a name drawn at the app's size over
    // it would cover the notes it belongs to. It shrinks with the drawing down
    // to the size letters stop being letters at, and is left off below that:
    // the canvas is named and counted for a reader either way, and every note
    // it draws is a row in Links or Links to this note.
    const labelSize = paint.fontSize * (view.scale < 1 ? view.scale : 1);
    if (focus && name && labelSize >= LABEL_MIN_SIZE) {
      ctx.font = `${labelSize}px ${paint.face}`;
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      ctx.fillStyle = paint.label;
      const room = size.width - VIEW_PADDING * 2;
      const baseline = Math.min(focus.y + LABEL_OFFSET, size.height - LABEL_OFFSET * 2);
      // The name lands wherever the note settled, which is sometimes over a
      // link. Laying the ground back down around the letters keeps it legible
      // without a plate to draw or a second colour to pick.
      ctx.lineWidth = LABEL_HALO;
      ctx.lineJoin = "round";
      ctx.strokeStyle = paint.ground;
      ctx.strokeText(name, focus.x, baseline, room);
      ctx.fillText(name, focus.x, baseline, room);
    }
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
    state = beginLayout(props.nodes, props.edges, options(), seedFor(props.focusPath));
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

  createEffect(
    on(
      () => [props.focusPath, props.nodes, props.edges, props.options] as const,
      () => {
        if (canvas) restart();
      },
      { defer: true },
    ),
  );

  // Moving the drawing, taking it in and out and searching it all change what
  // is painted and none of them change where a note settled, so they repaint
  // rather than start the settle again.
  createEffect(
    on(
      () => [props.zoom, props.pan, props.dimmed, props.colors] as const,
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
        role="img"
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
          const next = nodeAt(placed, pointIn(element, event));
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
          if (!props.onZoomBy) return;
          event.preventDefault();
          props.onZoomBy(Math.exp(-event.deltaY * WHEEL_ZOOM));
        }}
        onClick={(event) => {
          const element = canvas;
          if (!element) return;
          // A drag that ends over a note is a drag, not a click on the note.
          const dragged = press?.dragging ?? false;
          press = null;
          if (dragged) return;
          const picked = nodeAt(placed, pointIn(element, event));
          if (picked !== null) props.onOpen(picked);
        }}
      />
      <p class="graph-hovered">{props.nodes.find((node) => node.path === hovered())?.name ?? ""}</p>
    </div>
  );
}
