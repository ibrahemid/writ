import { describe, it, expect } from "vitest";
import { formatBytes } from "../../lib/format-bytes";

describe("formatBytes", () => {
  it("renders bytes under 1 KB without decimals", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(999)).toBe("999 B");
  });

  it("counts a kilobyte as a thousand bytes, as the file manager does", () => {
    expect(formatBytes(1000)).toBe("1 KB");
    expect(formatBytes(1500)).toBe("1.5 KB");
    expect(formatBytes(1_000_000)).toBe("1 MB");
    expect(formatBytes(1_000_000_000)).toBe("1 GB");
  });

  it("uses one decimal below ten of a unit", () => {
    expect(formatBytes(5_200_000)).toBe("5.2 MB");
    expect(formatBytes(9_949_999)).toBe("9.9 MB");
  });

  it("drops the decimal at or above ten of a unit", () => {
    expect(formatBytes(9_999_999)).toBe("10 MB");
    expect(formatBytes(15_000)).toBe("15 KB");
    expect(formatBytes(20_000_000)).toBe("20 MB");
  });

  it("guards against negative or non-finite input", () => {
    expect(formatBytes(-1)).toBe("0 B");
    expect(formatBytes(Number.NaN)).toBe("0 B");
  });
});
