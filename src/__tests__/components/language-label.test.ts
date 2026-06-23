import { describe, it, expect } from "vitest";
import { languageLabel } from "../../components/Editor/language-label";

describe("languageLabel", () => {
  it("maps known ids to their canonical casing", () => {
    expect(languageLabel("markdown")).toBe("Markdown");
    expect(languageLabel("typescript")).toBe("TypeScript");
    expect(languageLabel("json")).toBe("JSON");
    expect(languageLabel("php")).toBe("PHP");
  });

  it("falls back to Plain Text for an unset language", () => {
    expect(languageLabel(null)).toBe("Plain Text");
    expect(languageLabel(undefined)).toBe("Plain Text");
    expect(languageLabel("")).toBe("Plain Text");
  });

  it("title-cases an unknown id rather than dropping it", () => {
    expect(languageLabel("kotlin")).toBe("Kotlin");
  });
});
