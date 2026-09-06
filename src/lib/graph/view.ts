/**
 * How a settled drawing is fitted into the room the canvas has.
 *
 * The settle works in world units and knows nothing about the panel it ends up
 * in (`layout.ts`); this file is the other half of that split, and turns a set
 * of world positions into screen ones. Every note keeps its place relative to
 * every other note: one scale for both axes, so nothing is squashed, and one
 * shift, so what was settled in the middle is drawn in the middle.
 */

import type { PlacedNode, Point } from "./layout";

/** The room a drawing is fitted into, in CSS pixels. */
export interface Viewport {
  width: number;
  height: number;
  /** How far the drawing stays from the edge. */
  padding: number;
}

/** A scale and a shift, applied to every note the same way. */
export interface View {
  scale: number;
  offsetX: number;
  offsetY: number;
}

/**
 * How far a drawing is opened out past the size it settled at.
 *
 * A note with one neighbour settles into a shape a link long, and blowing that
 * up until it touches the edges would make two notes read as a whole folder.
 */
const MAX_SCALE = 1.6;

/** Below this a span counts as nothing: one note, or a set sitting on a point. */
const NO_SPAN = 1e-9;

/**
 * Where a settled set is drawn, given the room to draw it in.
 *
 * The scale is what fits the notes and their discs inside the room with the
 * padding left clear, and it is the smaller of the two axes' scales, so the
 * drawing keeps its proportions.
 */
export function fitToView(placed: readonly PlacedNode[], viewport: Viewport): View {
  const middle = { scale: 1, offsetX: viewport.width / 2, offsetY: viewport.height / 2 };
  if (placed.length === 0) return middle;

  let lowX = placed[0].x;
  let highX = placed[0].x;
  let lowY = placed[0].y;
  let highY = placed[0].y;
  let radius = placed[0].radius;
  for (const node of placed) {
    if (node.x < lowX) lowX = node.x;
    if (node.x > highX) highX = node.x;
    if (node.y < lowY) lowY = node.y;
    if (node.y > highY) highY = node.y;
    if (node.radius > radius) radius = node.radius;
  }

  // A disc is drawn at the size it is read at, whatever the drawing is scaled
  // to, so the room the positions are fitted into is the room left over once
  // the outermost discs have theirs.
  const roomX = viewport.width - viewport.padding * 2 - radius * 2;
  const roomY = viewport.height - viewport.padding * 2 - radius * 2;
  const spanX = highX - lowX;
  const spanY = highY - lowY;

  let scale = MAX_SCALE;
  if (spanX > NO_SPAN && roomX / spanX < scale) scale = roomX / spanX;
  if (spanY > NO_SPAN && roomY / spanY < scale) scale = roomY / spanY;
  if (!(scale > 0)) scale = 1;

  const midX = (lowX + highX) / 2;
  const midY = (lowY + highY) / 2;
  return {
    scale,
    offsetX: viewport.width / 2 - midX * scale,
    offsetY: viewport.height / 2 - midY * scale,
  };
}

/** One world point, on screen. */
export function toScreen(point: Point, view: View): Point {
  return { x: point.x * view.scale + view.offsetX, y: point.y * view.scale + view.offsetY };
}
