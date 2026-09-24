import { describe, expect, it } from "vitest";
import { dedupe, firstLineTitle, mintedStem, sanitizeTitle } from "../backend/naming";

describe("the demo's naming rules", () => {
  it("mints writ-<yymmdd>-<hhmm> on the local clock", () => {
    expect(mintedStem(new Date(2026, 8, 21, 7, 48))).toBe("writ-260921-0748");
  });

  it("takes a heading or list marker off the first line", () => {
    expect(firstLineTitle("# Grocery list\nmilk")).toBe("Grocery list");
    expect(firstLineTitle("- Call the plumber")).toBe("Call the plumber");
  });

  it("names nothing from a fence, a lone link, markers or a blank line", () => {
    expect(firstLineTitle("---\ntitle: x")).toBeNull();
    expect(firstLineTitle("[[Other note]]")).toBeNull();
    expect(firstLineTitle("## ")).toBeNull();
    expect(firstLineTitle("\nlater")).toBeNull();
  });

  it("maps path characters to spaces and trims dots", () => {
    expect(sanitizeTitle("a/b:c")).toBe("a b c");
    expect(sanitizeTitle("..hidden..")).toBe("hidden");
    expect(sanitizeTitle(" / ")).toBeNull();
  });

  it("dedupes with a counter", () => {
    const taken = new Set(["Note", "Note-2"]);
    expect(dedupe("Note", (c) => taken.has(c))).toBe("Note-3");
    expect(dedupe("Fresh", (c) => taken.has(c))).toBe("Fresh");
  });
});
