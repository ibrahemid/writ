import { describe, it, expect } from "vitest";
import { isImeComposing } from "../../lib/ime";

describe("isImeComposing", () => {
  it("reads the composing flag", () => {
    expect(isImeComposing({ isComposing: true, keyCode: 13 })).toBe(true);
  });

  it("reads keyCode 229, which WebKit sends with the Enter that confirms a conversion", () => {
    expect(isImeComposing({ isComposing: false, keyCode: 229 })).toBe(true);
  });

  it("lets an ordinary Enter through", () => {
    expect(isImeComposing({ isComposing: false, keyCode: 13 })).toBe(false);
  });
});
