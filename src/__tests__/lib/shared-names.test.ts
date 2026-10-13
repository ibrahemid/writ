import { describe, it, expect } from "vitest";
import { foldersForSharedNames } from "../../lib/shared-names";

describe("foldersForSharedNames", () => {
  it("names the folder of each file whose name another file shows", () => {
    const folders = foldersForSharedNames([
      "/Users/me/Writ/A/Plan.md",
      "/Users/me/Writ/B/Plan.md",
      "/Users/me/Writ/Shopping.md",
    ]);

    expect(Object.fromEntries(folders)).toEqual({
      "/Users/me/Writ/A/Plan.md": "A",
      "/Users/me/Writ/B/Plan.md": "B",
    });
  });

  it("gives a name no other file shows nothing", () => {
    const folders = foldersForSharedNames(["/Users/me/Writ/Plan.md", "/Users/me/Writ/Plans.md"]);

    expect(folders.size).toBe(0);
    expect(foldersForSharedNames([]).size).toBe(0);
  });

  it("takes the shown name, so two formats of one name are told apart", () => {
    const folders = foldersForSharedNames([
      "/Users/me/Writ/Plan.md",
      "/Users/me/Writ/Work/Plan.txt",
    ]);

    expect(folders.get("/Users/me/Writ/Plan.md")).toBe("Writ");
    expect(folders.get("/Users/me/Writ/Work/Plan.txt")).toBe("Work");
  });

  it("climbs as many folders as it takes when the folders share a name too", () => {
    const folders = foldersForSharedNames([
      "/Users/me/Writ/A/Drafts/Plan.md",
      "/Users/me/Writ/B/Drafts/Plan.md",
      "/Users/me/Writ/C/Plan.md",
    ]);

    expect(folders.get("/Users/me/Writ/A/Drafts/Plan.md")).toBe("A/Drafts");
    expect(folders.get("/Users/me/Writ/B/Drafts/Plan.md")).toBe("B/Drafts");
    expect(folders.get("/Users/me/Writ/C/Plan.md")).toBe("Writ/C");
  });

  it("tells a file apart from one deeper under a folder of the same name", () => {
    const folders = foldersForSharedNames(["/Writ/Plan.md", "/Writ/Old/Writ/Plan.md"]);

    expect(folders.get("/Writ/Plan.md")).toBe("Writ");
    expect(folders.get("/Writ/Old/Writ/Plan.md")).toBe("Old/Writ");
  });

  it("reads Windows paths the same way", () => {
    const folders = foldersForSharedNames([
      "C:\\Users\\me\\Writ\\A\\Plan.md",
      "C:\\Users\\me\\Writ\\B\\Plan.md",
    ]);

    expect(folders.get("C:\\Users\\me\\Writ\\A\\Plan.md")).toBe("A");
    expect(folders.get("C:\\Users\\me\\Writ\\B\\Plan.md")).toBe("B");
  });
});
