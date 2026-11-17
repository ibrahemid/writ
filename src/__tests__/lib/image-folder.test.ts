import { describe, it, expect } from "vitest";
import { DEFAULT_IMAGE_FOLDER, readImageFolder } from "../../lib/image-folder";

describe("readImageFolder", () => {
  it("keeps a folder inside the file's folder", () => {
    expect(readImageFolder("assets")).toEqual({ ok: true, folder: "assets" });
    expect(readImageFolder(" media/shots ")).toEqual({ ok: true, folder: "media/shots" });
  });

  it("writes backslashes as forward slashes and drops empty parts", () => {
    expect(readImageFolder("media\\shots\\")).toEqual({ ok: true, folder: "media/shots" });
    expect(readImageFolder("media//shots")).toEqual({ ok: true, folder: "media/shots" });
  });

  it("reads an empty value as the file's own folder", () => {
    expect(readImageFolder("")).toEqual({ ok: true, folder: "" });
    expect(readImageFolder("   ")).toEqual({ ok: true, folder: "" });
  });

  it("rejects an absolute path", () => {
    expect(readImageFolder("/Users/me/images")).toEqual({ ok: false, reason: "absolute" });
    expect(readImageFolder("\\\\server\\share")).toEqual({ ok: false, reason: "absolute" });
    expect(readImageFolder("C:\\images")).toEqual({ ok: false, reason: "absolute" });
  });

  it("rejects a folder that leaves the file's folder", () => {
    expect(readImageFolder("../images")).toEqual({ ok: false, reason: "leaves_the_folder" });
    expect(readImageFolder("assets/./shots")).toEqual({ ok: false, reason: "leaves_the_folder" });
  });

  it("names assets as the folder when none is set", () => {
    expect(DEFAULT_IMAGE_FOLDER).toBe("assets");
  });
});
