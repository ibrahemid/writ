// @vitest-environment node
import { tmpdir } from "node:os";
import { join } from "node:path";
import { normalizePath, resolveConfig } from "vite";
import { describe, expect, it } from "vitest";
import { APP_ROOT, HARNESS_PAGE, ROOT, harnessBuildConfig } from "../harness.mjs";

describe("perf harness build", () => {
  it("builds the harness page alone, not the app's pages", async () => {
    const config = await resolveConfig(harnessBuildConfig(join(tmpdir(), "writ-perf-harness")), "build");
    expect(config.root).toBe(normalizePath(APP_ROOT));
    expect(config.build.rolldownOptions.input).toBe(HARNESS_PAGE);
  });

  it("leaves the app build with the main and capture pages", async () => {
    const config = await resolveConfig({ configFile: join(ROOT, "vite.config.ts"), logLevel: "error" }, "build");
    expect(config.build.rolldownOptions.input).toEqual({
      main: join(ROOT, "index.html"),
      capture: join(ROOT, "capture.html"),
    });
  });
});
