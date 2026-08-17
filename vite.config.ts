/// <reference types="vitest" />
import { defineConfig } from "vite";
import solidPlugin from "vite-plugin-solid";

const host = process.env.TAURI_DEV_HOST;
const devPort = Number(process.env.WRIT_DEV_PORT) || 1420;

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
    exclude: ["**/node_modules/**", "**/dist/**", "**/.claude/**", "site/**"],
  },
});
