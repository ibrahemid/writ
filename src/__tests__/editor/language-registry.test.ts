import { describe, it, expect, beforeEach } from "vitest";
import {
  register,
  getExtension,
  listLanguageIds,
  unregisterAll,
} from "../../editor/language-registry";

describe("language-registry", () => {
  beforeEach(() => {
    unregisterAll();
  });

  it("returns an empty extension array for an unregistered id", () => {
    const ext = getExtension("does-not-exist");
    expect(Array.isArray(ext)).toBe(true);
    expect(ext as unknown as unknown[]).toHaveLength(0);
  });

  it("returns an empty extension array for null", () => {
    const ext = getExtension(null);
    expect(Array.isArray(ext)).toBe(true);
    expect(ext as unknown as unknown[]).toHaveLength(0);
  });

  it("returns the factory result for a registered id", () => {
    const marker = Symbol("marker");
    register("custom", () => marker as unknown as never);
    const ext = getExtension("custom");
    expect(ext).toBe(marker);
  });

  it("replaces a previous registration for the same id", () => {
    const first = Symbol("first");
    const second = Symbol("second");
    register("dup", () => first as unknown as never);
    register("dup", () => second as unknown as never);
    expect(getExtension("dup")).toBe(second);
  });

  it("listLanguageIds returns every registered id", () => {
    register("a", () => [] as unknown as never);
    register("b", () => [] as unknown as never);
    register("c", () => [] as unknown as never);
    expect(listLanguageIds().sort()).toEqual(["a", "b", "c"]);
  });
});

describe("builtin language registration", () => {
  beforeEach(() => {
    unregisterAll();
  });

  it("registers all nine v1 languages on the first call and is idempotent", async () => {
    const { registerBuiltinLanguages } = await import("../../editor/builtins");
    registerBuiltinLanguages();
    const first = listLanguageIds().sort();
    expect(first).toEqual([
      "css",
      "html",
      "javascript",
      "json",
      "markdown",
      "php",
      "python",
      "rust",
      "typescript",
    ]);
    registerBuiltinLanguages();
    expect(listLanguageIds().sort()).toEqual(first);
  });
});
