import { describe, it, expect } from "vitest";

import {
  ENCODING_CHOICES,
  encodingChoiceOf,
  encodingLabel,
  encodingName,
} from "../../lib/text-encoding";

describe("text encodings", () => {
  it("shows the status bar's short name for the encodings Rust names", () => {
    expect(encodingLabel("UTF-8")).toBe("UTF-8");
    expect(encodingLabel("UTF-8 BOM")).toBe("UTF-8 with BOM");
    expect(encodingLabel("UTF-16LE BOM")).toBe("UTF-16 LE");
    expect(encodingLabel("UTF-16BE")).toBe("UTF-16 BE");
    expect(encodingLabel("windows-1252")).toBe("Windows-1252");
    expect(encodingLabel("Shift_JIS")).toBe("Shift JIS");
    expect(encodingLabel("GBK")).toBe("GBK");
  });

  it("shows an encoding outside the list under Rust's name", () => {
    expect(encodingLabel("x-mac-cyrillic")).toBe("x-mac-cyrillic");
  });

  it("finds the choice for a name with or without its mark, in any case", () => {
    expect(encodingName("UTF-16LE BOM")).toBe("UTF-16LE");
    expect(encodingChoiceOf("UTF-16LE BOM")?.name).toBe("UTF-16LE");
    expect(encodingChoiceOf("shift_jis")?.short).toBe("Shift JIS");
    expect(encodingChoiceOf("klingon")).toBeUndefined();
  });

  it("offers each encoding once, under a label that names its script", () => {
    const names = ENCODING_CHOICES.map((choice) => choice.name.toLowerCase());
    expect(new Set(names).size).toBe(names.length);
    expect(encodingChoiceOf("windows-1252")?.label).toBe("Western (Windows-1252)");
    expect(encodingChoiceOf("UTF-8")?.label).toBe("UTF-8");
  });
});
