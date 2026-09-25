import { describe, expect, it } from "vitest";
import {
  dedupeStem,
  deriveFirstLineTitle,
  deriveNoteFileStem,
  formatDateStem,
  formatDottedStamp,
  formatMintedStem,
  padTwo,
  sanitizeTitle,
  scoreFuzzyMatch,
} from "../backend/naming";

describe("the demo's naming rules", () => {
  it("mints writ-<yymmdd>-<hhmm> on the local clock", () => {
    expect(formatMintedStem(new Date(2026, 8, 21, 7, 48))).toBe("writ-260921-0748");
  });

  it("takes a heading or list marker off the first line", () => {
    expect(deriveFirstLineTitle("# Grocery list\nmilk")).toBe("Grocery list");
    expect(deriveFirstLineTitle("- Call the plumber")).toBe("Call the plumber");
  });

  it("names nothing from a fence, a lone link, markers or a blank line", () => {
    expect(deriveFirstLineTitle("---\ntitle: x")).toBeNull();
    expect(deriveFirstLineTitle("[[Other note]]")).toBeNull();
    expect(deriveFirstLineTitle("## ")).toBeNull();
    expect(deriveFirstLineTitle("\nlater")).toBeNull();
  });

  it("maps path characters to spaces and trims dots", () => {
    expect(sanitizeTitle("a/b:c")).toBe("a b c");
    expect(sanitizeTitle("..hidden..")).toBe("hidden");
    expect(sanitizeTitle(" / ")).toBeNull();
  });

  it("dedupes with a counter", () => {
    const taken = new Set(["Note", "Note-2"]);
    expect(dedupeStem("Note", (c) => taken.has(c))).toBe("Note-3");
    expect(dedupeStem("Fresh", (c) => taken.has(c))).toBe("Fresh");
  });

  it("writes dates and stamps the way chrono pads them", () => {
    const at = new Date(2026, 0, 5, 7, 8, 9);
    expect(padTwo(7)).toBe("07");
    expect(padTwo(12)).toBe("12");
    expect(formatDateStem(at)).toBe("2026-01-05");
    expect(formatDottedStamp(at)).toBe("2026-01-05 07.08.09");
  });

  it("dates a note file whose title names nothing", () => {
    const at = new Date(2026, 8, 21, 7, 48);
    expect(deriveNoteFileStem("Seed swap", at)).toBe("Seed swap");
    expect(deriveNoteFileStem("  ", at)).toBe("2026-09-21");
    expect(deriveNoteFileStem("writ-260921-0748", at)).toBe("2026-09-21");
    expect(deriveNoteFileStem("/", at)).toBe("2026-09-21");
  });
});

describe("scoreFuzzyMatch", () => {
  it("scores a subsequence, three per adjacent letter, one per gap, ten for a prefix", () => {
    expect(scoreFuzzyMatch("Garden plan.md", "gar")).toBe(19);
    expect(scoreFuzzyMatch("Garden plan.md", "gp")).toBe(4);
  });

  it("ignores case and the spaces in the query", () => {
    expect(scoreFuzzyMatch("Garden plan.md", "G P")).toBe(scoreFuzzyMatch("garden plan.md", "gp"));
  });

  it("ranks letters in a run above the same letters scattered", () => {
    const run = scoreFuzzyMatch("Seed order.md", "order");
    const scattered = scoreFuzzyMatch("Old red era.md", "order");
    expect(run).not.toBeNull();
    expect(scattered).not.toBeNull();
    expect(run as number).toBeGreaterThan(scattered as number);
  });

  it("matches nothing for an empty query or letters out of order", () => {
    expect(scoreFuzzyMatch("Garden plan.md", "")).toBeNull();
    expect(scoreFuzzyMatch("Garden plan.md", "   ")).toBeNull();
    expect(scoreFuzzyMatch("Garden plan.md", "pg")).toBeNull();
    expect(scoreFuzzyMatch("Garden plan.md", "xyz")).toBeNull();
  });
});
