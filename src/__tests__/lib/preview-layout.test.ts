import { describe, it, expect } from "vitest";
import {
  DEFAULT_RATIO,
  defaultSplit,
  layoutFromPersisted,
  layoutRatio,
  nextCycleLayout,
} from "../../lib/preview-layout";

describe("preview-layout helpers", () => {
  it("defaultSplit is a 50/50 vertical split", () => {
    expect(defaultSplit()).toEqual({ kind: "split", ratio: DEFAULT_RATIO, orientation: "vertical" });
  });

  it("cycle goes Source → Split → Preview → Source", () => {
    const a = nextCycleLayout({ kind: "source" });
    expect(a.kind).toBe("split");
    const b = nextCycleLayout(a);
    expect(b).toEqual({ kind: "preview" });
    const c = nextCycleLayout(b);
    expect(c).toEqual({ kind: "source" });
  });

  it("layoutFromPersisted parses each persisted kind", () => {
    expect(layoutFromPersisted("source", null)).toEqual({ kind: "source" });
    expect(layoutFromPersisted("preview", null)).toEqual({ kind: "preview" });
    expect(layoutFromPersisted("split", 0.7)).toEqual({
      kind: "split",
      ratio: 0.7,
      orientation: "vertical",
    });
  });

  it("layoutFromPersisted falls back to default ratio when missing", () => {
    expect(layoutFromPersisted("split", null)).toEqual({
      kind: "split",
      ratio: DEFAULT_RATIO,
      orientation: "vertical",
    });
  });

  it("layoutFromPersisted treats unknown kinds as source", () => {
    expect(layoutFromPersisted("detached", null)).toEqual({ kind: "source" });
    expect(layoutFromPersisted("bogus", null)).toEqual({ kind: "source" });
  });

  it("layoutRatio returns the split ratio or null otherwise", () => {
    expect(layoutRatio({ kind: "source" })).toBeNull();
    expect(layoutRatio({ kind: "preview" })).toBeNull();
    expect(layoutRatio(defaultSplit())).toBe(DEFAULT_RATIO);
  });
});
