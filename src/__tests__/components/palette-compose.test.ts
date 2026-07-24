import { describe, it, expect } from "vitest";
import { composeSections } from "../../components/Palette/compose";
import type { PaletteResult, ResultProvider } from "../../components/Palette/types";

function result(id: string, section?: { kind: string; label: string | null }): PaletteResult {
  return { id, label: id, section, execute: () => {} };
}

function provider(over: Partial<ResultProvider> & { id: string }): ResultProvider {
  return {
    section: over.id,
    order: 0,
    cap: Number.POSITIVE_INFINITY,
    query: () => [],
    ...over,
  };
}

describe("composeSections", () => {
  it("renders providers in ascending order regardless of array order", () => {
    const providers = [
      provider({ id: "content", order: 3 }),
      provider({ id: "commands", order: 0 }),
      provider({ id: "files", order: 2 }),
    ];
    const { sections } = composeSections(providers, {
      content: [result("c1")],
      commands: [result("k1")],
      files: [result("f1")],
    });
    expect(sections.map((s) => s.providerId)).toEqual(["commands", "files", "content"]);
  });

  it("caps a provider and reports the overflow instead of dropping it silently", () => {
    const providers = [provider({ id: "files", cap: 2 })];
    const { sections, flat } = composeSections(providers, {
      files: [result("a"), result("b"), result("c"), result("d")],
    });
    expect(flat).toHaveLength(2);
    expect(sections[0].hiddenCount).toBe(2);
    expect(sections[0].total).toBe(4);
  });

  it("keeps a noisy provider from crowding out a later one", () => {
    const providers = [
      provider({ id: "content", order: 1, cap: 2 }),
      provider({ id: "files", order: 2, cap: 2 }),
    ];
    const { flat } = composeSections(providers, {
      content: Array.from({ length: 50 }, (_, i) => result(`c${i}`)),
      files: [result("f0"), result("f1")],
    });
    expect(flat.map((r) => r.id)).toEqual(["c0", "c1", "f0", "f1"]);
  });

  it("omits a provider that produced nothing", () => {
    const providers = [provider({ id: "a" }), provider({ id: "b" })];
    const { sections } = composeSections(providers, { a: [result("a1")], b: [] });
    expect(sections.map((s) => s.providerId)).toEqual(["a"]);
  });

  it("splits one provider into runs by the section its rows carry", () => {
    const providers = [provider({ id: "commands", section: "Commands" })];
    const { sections } = composeSections(providers, {
      commands: [
        result("r1", { kind: "recent", label: "Recent" }),
        result("r2", { kind: "recent", label: "Recent" }),
        result("a1", { kind: "all", label: "Commands" }),
        result("e1", { kind: "all", label: "Editor" }),
      ],
    });
    expect(sections.map((s) => [s.kind, s.label, s.results.length])).toEqual([
      ["recent", "Recent", 2],
      ["all", "Commands", 1],
      ["all", "Editor", 1],
    ]);
  });

  it("falls back to the provider heading when a row carries no section", () => {
    const providers = [provider({ id: "settings", section: "Settings" })];
    const { sections } = composeSections(providers, { settings: [result("s1")] });
    expect(sections[0].kind).toBe("settings");
    expect(sections[0].label).toBe("Settings");
    expect(sections[0].ariaLabel).toBe("Settings");
  });

  it("renders no heading when the provider sets one to null but keeps the aria name", () => {
    const providers = [provider({ id: "commands", section: "Commands", heading: null })];
    const { sections } = composeSections(providers, { commands: [result("c1")] });
    expect(sections[0].label).toBeNull();
    expect(sections[0].ariaLabel).toBe("Commands");
  });

  it("flattens in visual order", () => {
    const providers = [
      provider({ id: "commands", order: 0 }),
      provider({ id: "files", order: 1 }),
    ];
    const { flat } = composeSections(providers, {
      commands: [result("k1"), result("k2")],
      files: [result("f1")],
    });
    expect(flat.map((r) => r.id)).toEqual(["k1", "k2", "f1"]);
  });
});
