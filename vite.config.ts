/// <reference types="vitest" />
import { defineConfig } from "vite";
import solidPlugin from "vite-plugin-solid";

const host = process.env.TAURI_DEV_HOST;
const devPort = Number(process.env.WRIT_DEV_PORT) || 1420;

const SHARED_EXCLUDE = ["**/node_modules/**", "**/dist/**", "**/.claude/**", "site/**"];
const MOUNT_TESTS = [
  "src/__tests__/components/**/*.{test,spec}.{ts,tsx}",
  "src/__tests__/editor/**/*.{test,spec}.{ts,tsx}",
];

export default defineConfig({
  plugins: [solidPlugin()],
  clearScreen: false,
  // Pinned to the webviews Writ ships against: tauri.conf.json sets
  // minimumSystemVersion 12.0, whose WKWebView predates Vite's default
  // baseline target.
  build: {
    target: ["es2020", "edge88", "firefox78", "chrome87", "safari14"],
  },
  server: {
    port: devPort,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: devPort + 1 } : undefined,
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test-setup.ts"],
    exclude: SHARED_EXCLUDE,
    projects: [
      {
        extends: true,
        test: {
          name: "unit",
          exclude: [...SHARED_EXCLUDE, ...MOUNT_TESTS],
        },
      },
      {
        // Mounting a component or an editor view can take longer than the 5s
        // default when the machine is busy; only these files get the longer
        // budget.
        extends: true,
        test: {
          name: "mount",
          include: MOUNT_TESTS,
          testTimeout: 15_000,
        },
      },
    ],
  },
});
