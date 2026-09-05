import { describe, it, expect, vi, afterEach } from "vitest";
import { detectPlatform, resolvePlatform } from "../../lib/platform";

// The three-shell screenshots are driven from a Mac, so a dev build takes the
// shell from the environment. A release build always reads the host.

afterEach(() => {
  vi.unstubAllEnvs();
});

describe("resolvePlatform", () => {
  it("honours the dev override", () => {
    for (const platform of ["mac", "win", "linux"] as const) {
      vi.stubEnv("VITE_WRIT_PLATFORM", platform);
      expect(resolvePlatform()).toBe(platform);
    }
  });

  it("ignores an override that names no shell", () => {
    vi.stubEnv("VITE_WRIT_PLATFORM", "sunos");
    expect(resolvePlatform()).toBe(detectPlatform());
  });

  it("falls back to the host in a production build", () => {
    vi.stubEnv("DEV", false);
    vi.stubEnv("VITE_WRIT_PLATFORM", "win");
    expect(resolvePlatform()).toBe(detectPlatform());
  });
});
