import { describe, it, expect } from "vitest";
import { foldersForSharedNames } from "../../lib/shared-names";

describe("foldersForSharedNames", () => {
  it("names the folder of each file whose name another file shows", () => {
    const folders = foldersForSharedNames([
      "/Users/me/Jotra/A/Plan.md",
      "/Users/me/Jotra/B/Plan.md",
      "/Users/me/Jotra/Shopping.md",
    ]);

    expect(Object.fromEntries(folders)).toEqual({
      "/Users/me/Jotra/A/Plan.md": "A",
      "/Users/me/Jotra/B/Plan.md": "B",
    });
  });

  it("gives a name no other file shows nothing", () => {
    const folders = foldersForSharedNames(["/Users/me/Jotra/Plan.md", "/Users/me/Jotra/Plans.md"]);

    expect(folders.size).toBe(0);
    expect(foldersForSharedNames([]).size).toBe(0);
  });

  it("takes the shown name, so two formats of one name are told apart", () => {
    const folders = foldersForSharedNames([
      "/Users/me/Jotra/Plan.md",
      "/Users/me/Jotra/Work/Plan.txt",
    ]);

    expect(folders.get("/Users/me/Jotra/Plan.md")).toBe("Jotra");
    expect(folders.get("/Users/me/Jotra/Work/Plan.txt")).toBe("Work");
  });

  it("climbs as many folders as it takes when the folders share a name too", () => {
    const folders = foldersForSharedNames([
      "/Users/me/Jotra/A/Drafts/Plan.md",
      "/Users/me/Jotra/B/Drafts/Plan.md",
      "/Users/me/Jotra/C/Plan.md",
    ]);

    expect(folders.get("/Users/me/Jotra/A/Drafts/Plan.md")).toBe("A/Drafts");
    expect(folders.get("/Users/me/Jotra/B/Drafts/Plan.md")).toBe("B/Drafts");
    expect(folders.get("/Users/me/Jotra/C/Plan.md")).toBe("Jotra/C");
  });

  it("tells a file apart from one deeper under a folder of the same name", () => {
    const folders = foldersForSharedNames(["/Jotra/Plan.md", "/Jotra/Old/Jotra/Plan.md"]);

    expect(folders.get("/Jotra/Plan.md")).toBe("Jotra");
    expect(folders.get("/Jotra/Old/Jotra/Plan.md")).toBe("Old/Jotra");
  });

  it("reads Windows paths the same way", () => {
    const folders = foldersForSharedNames([
      "C:\\Users\\me\\Jotra\\A\\Plan.md",
      "C:\\Users\\me\\Jotra\\B\\Plan.md",
    ]);

    expect(folders.get("C:\\Users\\me\\Jotra\\A\\Plan.md")).toBe("A");
    expect(folders.get("C:\\Users\\me\\Jotra\\B\\Plan.md")).toBe("B");
  });
});
