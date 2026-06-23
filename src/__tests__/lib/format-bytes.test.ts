import { describe, it, expect } from "vitest";
import { formatBytes } from "../../lib/format-bytes";

describe("formatBytes", () => {
  it("renders bytes under 1 KB without decimals", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1023)).toBe("1023 B");
  });

  it("uses one decimal below ten of a unit", () => {
    expect(formatBytes(1024)).toBe("1 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(5_242_880)).toBe("5 MB");
  });

  it("drops the decimal at or above ten of a unit", () => {
    expect(formatBytes(15 * 1024)).toBe("15 KB");
    expect(formatBytes(20 * 1024 * 1024)).toBe("20 MB");
  });

  it("guards against negative or non-finite input", () => {
    expect(formatBytes(-1)).toBe("0 B");
    expect(formatBytes(Number.NaN)).toBe("0 B");
  });
});
