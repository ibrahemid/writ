import { join } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = fileURLToPath(new URL("../..", import.meta.url));
export const APP_ROOT = join(ROOT, "scripts/perf-editor/app");
export const HARNESS_PAGE = join(APP_ROOT, "index.html");

export function harnessBuildConfig(outDir) {
  return {
    configFile: join(ROOT, "vite.config.ts"),
    root: APP_ROOT,
    base: "./",
    logLevel: "error",
    build: {
      outDir,
      emptyOutDir: true,
      sourcemap: false,
      reportCompressedSize: false,
      // A string replaces the app's page map from vite.config.ts; an object or array would be merged into it.
      rolldownOptions: { input: HARNESS_PAGE },
    },
  };
}
