import { describe, it, expect } from "vitest";
import { parsePaletteQuery } from "../../components/Palette/query";

describe("parsePaletteQuery", () => {
  it("routes an unprefixed query to every provider", () => {
    expect(parsePaletteQuery("main.rs")).toEqual({ mode: "all", text: "main.rs", prefix: "" });
  });

  it("routes '>' to commands", () => {
    expect(parsePaletteQuery(">save")).toEqual({ mode: "commands", text: "save", prefix: ">" });
  });

  it("routes '#' to content", () => {
    expect(parsePaletteQuery("#todo")).toEqual({ mode: "content", text: "todo", prefix: "#" });
  });

  it("routes ':' to go to line", () => {
    expect(parsePaletteQuery(":42")).toEqual({ mode: "line", text: "42", prefix: ":" });
  });

  it("trims around the prefix", () => {
    expect(parsePaletteQuery("  >  save  ")).toEqual({
      mode: "commands",
      text: "save",
      prefix: ">",
    });
  });

  it("treats a bare prefix as that mode with an empty query", () => {
    expect(parsePaletteQuery("#")).toEqual({ mode: "content", text: "", prefix: "#" });
  });

  it("keeps a prefix that is not at the start as ordinary text", () => {
    expect(parsePaletteQuery("a > b")).toEqual({ mode: "all", text: "a > b", prefix: "" });
  });

  it("treats an empty query as the unprefixed mode", () => {
    expect(parsePaletteQuery("")).toEqual({ mode: "all", text: "", prefix: "" });
  });
});
