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

  it("cycles a non-markdown buffer through source, split and preview", () => {
    const a = nextCycleLayout({ kind: "source" }, "html");
    expect(a.kind).toBe("split");
    const b = nextCycleLayout(a, "html");
    expect(b).toEqual({ kind: "preview" });
    const c = nextCycleLayout(b, "html");
    expect(c).toEqual({ kind: "source" });
  });

  it("cycles a markdown buffer between inline and source", () => {
    const a = nextCycleLayout({ kind: "inline" }, "markdown");
    expect(a).toEqual({ kind: "source" });
    const b = nextCycleLayout(a, "markdown");
    expect(b).toEqual({ kind: "inline" });
  });

  it("layoutFromPersisted parses each persisted kind", () => {
    expect(layoutFromPersisted("source", null, "html")).toEqual({ kind: "source" });
    expect(layoutFromPersisted("preview", null, "html")).toEqual({ kind: "preview" });
    expect(layoutFromPersisted("split", 0.7, "html")).toEqual({
      kind: "split",
      ratio: 0.7,
      orientation: "vertical",
    });
  });

  it("layoutFromPersisted falls back to default ratio when missing", () => {
    expect(layoutFromPersisted("split", null, "html")).toEqual({
      kind: "split",
      ratio: DEFAULT_RATIO,
      orientation: "vertical",
    });
  });

  it("layoutFromPersisted treats unknown kinds as source", () => {
    expect(layoutFromPersisted("detached", null, "html")).toEqual({ kind: "source" });
    expect(layoutFromPersisted("bogus", null, "html")).toEqual({ kind: "source" });
  });

  it("reads a persisted split layout on a markdown buffer as inline", () => {
    expect(layoutFromPersisted("split", 0.7, "markdown")).toEqual({ kind: "inline" });
  });

  it("reads a persisted preview layout on a markdown buffer as inline", () => {
    expect(layoutFromPersisted("preview", null, "markdown")).toEqual({ kind: "inline" });
  });

  it("leaves a persisted split layout on an html buffer as split", () => {
    expect(layoutFromPersisted("split", 0.7, "html")).toEqual({
      kind: "split",
      ratio: 0.7,
      orientation: "vertical",
    });
  });

  it("keeps a persisted source layout on a markdown buffer as source", () => {
    expect(layoutFromPersisted("source", null, "markdown")).toEqual({ kind: "source" });
  });

  it("layoutRatio returns the split ratio or null otherwise", () => {
    expect(layoutRatio({ kind: "source" })).toBeNull();
    expect(layoutRatio({ kind: "preview" })).toBeNull();
    expect(layoutRatio(defaultSplit())).toBe(DEFAULT_RATIO);
  });
});
