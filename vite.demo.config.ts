import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import solidPlugin from "vite-plugin-solid";

export default defineConfig({
  root: fileURLToPath(new URL("./demo", import.meta.url)),
  base: "/demo/",
  plugins: [solidPlugin()],
  server: {
    port: 1430,
    strictPort: true,
    fs: { allow: [fileURLToPath(new URL(".", import.meta.url))] },
  },
  build: {
    outDir: fileURLToPath(new URL("./site/public/demo", import.meta.url)),
    emptyOutDir: true,
    target: "es2022",
  },
});
