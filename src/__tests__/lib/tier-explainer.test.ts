import { describe, it, expect } from "vitest";
import { explainTier, tierName, type TierFacts } from "../../lib/tier-explainer";
import { MAX_NORMAL_LINE_LENGTH, WRAP_LINE_LIMIT } from "../../editor/large-file";

function facts(extra: Partial<TierFacts>): TierFacts {
  return {
    mode: null,
    longest: 0,
    overrides: {},
    markdown: false,
    spellingOn: true,
    offered: null,
    ...extra,
  };
}

describe("explainTier", () => {
  it("says nothing for a normal file with nothing to offer", () => {
    expect(explainTier(facts({}))).toBeNull();
  });

  it("lists what a large Markdown file turns off and offers wrapping and highlighting", () => {
    const explained = explainTier(facts({ mode: { kind: "LargeFile" }, markdown: true }));
    expect(explained?.chip).toBe("Large file");
    expect(explained?.notes).toEqual([
      "Files over 5 MB open in large-file mode.",
      "Off: Markdown formatting, spell check, links and text direction.",
      "Not in search results.",
    ]);
    expect(explained?.toggles).toEqual([
      { feature: "wrap", label: "Wrap lines", on: false },
      { feature: "highlight", label: "Highlight syntax", on: false },
    ]);
    expect(explained?.offer).toBeNull();
  });

  it("shows the person's choices as the toggles' state", () => {
    const explained = explainTier(
      facts({ mode: { kind: "LargeFile" }, overrides: { wrap: true, highlight: true } }),
    );
    expect(explained?.toggles.map((toggle) => toggle.on)).toEqual([true, true]);
  });

  it("leaves spell check out of the list when it is off everywhere", () => {
    const explained = explainTier(facts({ mode: { kind: "LargeFile" }, spellingOn: false }));
    expect(explained?.notes[1]).toBe("Off: links and text direction.");
  });

  it("explains a long-lined file per line and says when a line is past the wrap limit", () => {
    const short = explainTier(facts({ mode: { kind: "LongLines" }, longest: MAX_NORMAL_LINE_LENGTH + 1 }));
    expect(short?.chip).toBe("Long lines");
    expect(short?.notes).toEqual([
      "A line is over 10,000 characters.",
      "Lines that long show without links and text direction.",
      "Spell check is off for this file.",
    ]);
    expect(short?.toggles.map((toggle) => toggle.on)).toEqual([true, true]);

    const blob = explainTier(facts({ mode: { kind: "LongLines" }, longest: WRAP_LINE_LIMIT + 1, markdown: true }));
    expect(blob?.notes).toContain("Lines that long show without formatting, links and text direction.");
    expect(blob?.notes).toContain("A line over 1,000,000 characters starts with wrapping and highlighting off.");
    expect(blob?.toggles.map((toggle) => toggle.on)).toEqual([false, false]);
  });

  it("offers large-file mode once a normal file grew past 5 MB", () => {
    const explained = explainTier(facts({ offered: { kind: "LargeFile" } }));
    expect(explained?.chip).toBe("Over 5 MB");
    expect(explained?.offer).toBe("Use large-file mode");
    expect(explained?.toggles).toEqual([]);
  });

  it("offers the full editor once a large file is cut under 5 MB", () => {
    const explained = explainTier(facts({ mode: { kind: "LargeFile" }, offered: { kind: "Normal" } }));
    expect(explained?.chip).toBe("Large file");
    expect(explained?.offer).toBe("Use the full editor");
  });

  it("explains a binary file and offers nothing", () => {
    const explained = explainTier(facts({ mode: { kind: "Binary" } }));
    expect(explained?.chip).toBe("Binary, read-only");
    expect(explained?.toggles).toEqual([]);
  });
});

describe("tierName", () => {
  it("names the tiers a tooltip shows and none for a normal file", () => {
    expect(tierName({ kind: "LargeFileConfirm" })).toBe("Large file");
    expect(tierName({ kind: "LongLines" })).toBe("Long lines");
    expect(tierName({ kind: "Normal" })).toBeNull();
    expect(tierName(null)).toBeNull();
  });
});
