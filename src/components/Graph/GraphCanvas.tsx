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
import type { NeighbourhoodNode } from "../../lib/graph/neighbourhood";
import "./GraphCanvas.css";

interface Props {
  nodes: NeighbourhoodNode[];
  edges: LayoutEdge[];
  /** The open note, drawn filled while everything around it is drawn hollow. */
  focusPath: string;
  onOpen: (path: string) => void;
}

/** How much of a step's settle runs per frame. */
const STEPS_PER_FRAME = 8;

/** The smallest disc, and how much each extra link adds, in CSS pixels. */
const RADIUS_BASE = 3.5;
const RADIUS_PER_LINK = 0.9;
const RADIUS_MAX = 7;

/** How faint a link is drawn against the notes it joins. */
const EDGE_ALPHA = 0.4;

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
} as const;

type Palette = Record<keyof typeof TOKENS, string> & {
  /** The canvas element's own face and size, so the label follows the app's. */
  font: string;
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

/**
 * The notes around the open one, drawn.
 *
 * The open note is the one filled disc and the only thing wearing the accent;
 * its neighbours are hollow, and a link is a hairline quiet enough that a
 * dense note still reads as a shape rather than a mesh. The names are not
 * painted on: at this width they would collide, and the notes are already
 * listed as text in the section above, so the drawing says only where a note
 * sits and hovering one says which it is.
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
  let size = { width: DEFAULT_LAYOUT_OPTIONS.width, height: DEFAULT_LAYOUT_OPTIONS.height };

  const [hovered, setHovered] = createSignal<string | null>(null);

  const focusNode = () => props.nodes.find((node) => node.path === props.focusPath) ?? null;
  const neighbourCount = () => Math.max(0, props.nodes.length - 1);

  /** What the drawing is, for a reader who is never shown the drawing. */
  const description = () => {
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
      font: `${style.fontSize} ${style.fontFamily}`,
    };
  }

  function optionsFor(): LayoutOptions {
    return { ...DEFAULT_LAYOUT_OPTIONS, width: size.width, height: size.height };
  }

  function draw() {
    const element = canvas;
    const ctx = context;
    if (!element || !ctx || !palette || !state) return;

    const paint = palette;
    const points = positions(state);
    placed = props.nodes.map((node) => {
      const point = points.get(node.path) ?? { x: size.width / 2, y: size.height / 2 };
      return {
        path: node.path,
        x: point.x,
        y: point.y,
        radius: node.path === props.focusPath ? RADIUS_MAX : radiusFor(node.degree),
      };
    });
    const by = new Map(placed.map((node) => [node.path, node] as const));

    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, element.width, element.height);
    const ratio = element.width / size.width || 1;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);

    ctx.fillStyle = paint.ground;
    ctx.fillRect(0, 0, size.width, size.height);

    ctx.globalAlpha = EDGE_ALPHA;
    ctx.strokeStyle = paint.edge;
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (const edge of props.edges) {
      const from = by.get(edge.from);
      const to = by.get(edge.to);
      if (!from || !to) continue;
      ctx.moveTo(from.x, from.y);
      ctx.lineTo(to.x, to.y);
    }
    ctx.stroke();
    ctx.globalAlpha = 1;

    const under = hovered();
    for (const node of placed) {
      const isFocus = node.path === props.focusPath;
      ctx.beginPath();
      ctx.arc(node.x, node.y, node.radius, 0, Math.PI * 2);
      ctx.fillStyle = isFocus ? paint.focus : paint.nodeFill;
      ctx.fill();
      if (!isFocus) {
        ctx.lineWidth = node.path === under ? 1.5 : 1;
        ctx.strokeStyle = node.path === under ? paint.edge : paint.ring;
        ctx.stroke();
      }
    }

    const focus = by.get(props.focusPath);
    const name = focusNode()?.name;
    if (focus && name) {
      ctx.font = paint.font;
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      ctx.fillStyle = paint.label;
      const room = size.width - DEFAULT_LAYOUT_OPTIONS.padding * 2;
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

  function runFrame() {
    frame = null;
    if (!state) return;
    for (let i = 0; i < STEPS_PER_FRAME && !state.done; i += 1) state = step(state);
    draw();
    if (!state.done) frame = requestAnimationFrame(runFrame);
  }

  /**
   * Starts the settle again, which is what a new note, a new size or a first
   * paint all are. Reduced motion runs the whole settle here and paints the
   * answer once: no frame is asked for, so there is nothing to see moving.
   */
  function restart() {
    cancelFrame();
    if (props.nodes.length === 0) return;
    state = beginLayout(props.nodes, props.edges, optionsFor(), seedFor(props.focusPath));
    if (settlesAtOnce()) {
      while (!state.done) state = step(state);
      draw();
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
    const width = rect.width || DEFAULT_LAYOUT_OPTIONS.width;
    const height = rect.height || DEFAULT_LAYOUT_OPTIONS.height;
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
      () => [props.focusPath, props.nodes, props.edges] as const,
      () => {
        if (canvas) restart();
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
    <div class="graph">
      <canvas
        class="graph-canvas"
        classList={{ "is-over": hovered() !== null }}
        role="img"
        aria-label={description()}
        ref={canvas}
        onPointerMove={(event) => {
          const element = canvas;
          if (!element) return;
          const next = nodeAt(placed, pointIn(element, event));
          if (next !== hovered()) {
            setHovered(next);
            draw();
          }
        }}
        onPointerLeave={() => {
          if (hovered() !== null) {
            setHovered(null);
            draw();
          }
        }}
        onClick={(event) => {
          const element = canvas;
          if (!element) return;
          const picked = nodeAt(placed, pointIn(element, event));
          if (picked !== null) props.onOpen(picked);
        }}
      />
      <p class="graph-hovered">{props.nodes.find((node) => node.path === hovered())?.name ?? ""}</p>
    </div>
  );
}
