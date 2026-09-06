import { describe, it, expect } from "vitest";
import { fitToView, toScreen, type Viewport } from "../../lib/graph/view";
import type { PlacedNode } from "../../lib/graph/layout";

// The settle answers where the notes sit relative to each other; this answers
// where that goes on a canvas of a given size. What it may never do is change
// the drawing's proportions, because two notes squashed together on one axis
// are two notes the settle held apart and the canvas put back on top of each
// other.

const VIEWPORT: Viewport = { width: 216, height: 160, padding: 14 };

function nodes(points: { x: number; y: number }[], radius = 7): PlacedNode[] {
  return points.map((point, at) => ({ path: `n${at}.md`, ...point, radius }));
}

function drawn(placed: PlacedNode[], viewport = VIEWPORT) {
  const view = fitToView(placed, viewport);
  return placed.map((node) => ({ ...toScreen(node, view), radius: node.radius }));
}

function inside(point: { x: number; y: number; radius: number }, viewport = VIEWPORT): boolean {
  return (
    point.x - point.radius >= viewport.padding - 1e-6 &&
    point.x + point.radius <= viewport.width - viewport.padding + 1e-6 &&
    point.y - point.radius >= viewport.padding - 1e-6 &&
    point.y + point.radius <= viewport.height - viewport.padding + 1e-6
  );
}

describe("fitting a drawing into the canvas", () => {
  it("puts a wide drawing inside the canvas, discs and all", () => {
    const placed = nodes([
      { x: -900, y: -20 },
      { x: 0, y: 40 },
      { x: 900, y: -10 },
    ]);
    for (const point of drawn(placed)) expect(inside(point)).toBe(true);
  });

  it("puts a tall drawing inside the canvas, discs and all", () => {
    const placed = nodes([
      { x: -10, y: -700 },
      { x: 30, y: 0 },
      { x: -5, y: 700 },
    ]);
    for (const point of drawn(placed)) expect(inside(point)).toBe(true);
  });

  it("scales both axes the same, so nothing is squashed", () => {
    for (const placed of [
      nodes([
        { x: -900, y: -20 },
        { x: 0, y: 20 },
        { x: 900, y: 0 },
      ]),
      nodes([
        { x: -20, y: -900 },
        { x: 20, y: 0 },
        { x: 0, y: 900 },
      ]),
    ]) {
      const view = fitToView(placed, VIEWPORT);
      const world = Math.sqrt(
        (placed[1].x - placed[0].x) ** 2 + (placed[1].y - placed[0].y) ** 2,
      );
      const screen = drawn(placed);
      const shown = Math.sqrt(
        (screen[1].x - screen[0].x) ** 2 + (screen[1].y - screen[0].y) ** 2,
      );
      expect(shown).toBeCloseTo(world * view.scale, 6);
    }
  });

  it("centres what it fits", () => {
    const screen = drawn(
      nodes([
        { x: 400, y: 300 },
        { x: 800, y: 700 },
      ]),
    );
    const midX = (screen[0].x + screen[1].x) / 2;
    const midY = (screen[0].y + screen[1].y) / 2;
    expect(midX).toBeCloseTo(VIEWPORT.width / 2, 6);
    expect(midY).toBeCloseTo(VIEWPORT.height / 2, 6);
  });

  it("does not blow a small drawing up to the edges", () => {
    const placed = nodes([
      { x: -10, y: 0 },
      { x: 10, y: 0 },
    ]);
    const view = fitToView(placed, VIEWPORT);
    expect(view.scale).toBeLessThanOrEqual(1.6);
    for (const point of drawn(placed)) expect(inside(point)).toBe(true);
  });

  it("draws one note, and notes on one point, in the middle", () => {
    for (const placed of [
      nodes([{ x: 123, y: -45 }]),
      nodes([
        { x: 8, y: 8 },
        { x: 8, y: 8 },
      ]),
    ]) {
      for (const point of drawn(placed)) {
        expect(point.x).toBeCloseTo(VIEWPORT.width / 2, 6);
        expect(point.y).toBeCloseTo(VIEWPORT.height / 2, 6);
      }
    }
  });

  it("draws nothing in the middle rather than working out a scale for it", () => {
    const view = fitToView([], VIEWPORT);
    expect(view.scale).toBe(1);
    expect(toScreen({ x: 0, y: 0 }, view)).toEqual({
      x: VIEWPORT.width / 2,
      y: VIEWPORT.height / 2,
    });
  });
});
