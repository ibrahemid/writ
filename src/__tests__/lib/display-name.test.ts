import { describe, it, expect } from "vitest";

import { displayFileName, renameSeed } from "../../lib/display-name";

describe("displayFileName", () => {
  it("drops the extension of a text format", () => {
    expect(displayFileName("meeting.md")).toBe("meeting");
    expect(displayFileName("meeting.markdown")).toBe("meeting");
    expect(displayFileName("meeting.txt")).toBe("meeting");
    expect(displayFileName("meeting.text")).toBe("meeting");
  });

  it("ignores the case the extension is written in", () => {
    expect(displayFileName("README.MD")).toBe("README");
    expect(displayFileName("notes.Txt")).toBe("notes");
  });

  it("keeps every other extension", () => {
    expect(displayFileName("vite.config.ts")).toBe("vite.config.ts");
    expect(displayFileName("data.json")).toBe("data.json");
    expect(displayFileName("index.html")).toBe("index.html");
    expect(displayFileName("report.mdx")).toBe("report.mdx");
  });

  it("drops only the last extension", () => {
    expect(displayFileName("archive.txt.md")).toBe("archive.txt");
  });

  it("leaves a name that has no stem whole", () => {
    expect(displayFileName(".md")).toBe(".md");
    expect(displayFileName(".txt")).toBe(".txt");
  });

  it("leaves a name with no extension alone", () => {
    expect(displayFileName("Makefile")).toBe("Makefile");
    expect(displayFileName("writ-260921-0748")).toBe("writ-260921-0748");
  });
});

describe("renameSeed", () => {
  it("opens with the whole name and selects the stem", () => {
    expect(renameSeed("meeting.md")).toEqual({ value: "meeting.md", selectionEnd: 7 });
  });

  it("selects the whole name when the extension is not a text one", () => {
    expect(renameSeed("data.json")).toEqual({ value: "data.json", selectionEnd: 9 });
  });

  it("selects the whole name when there is no extension", () => {
    expect(renameSeed("Makefile")).toEqual({ value: "Makefile", selectionEnd: 8 });
  });
});
