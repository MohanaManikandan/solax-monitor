import { defineConfig } from "vite";

// The dev server answers on its own port and hands every API request to the
// solax server running with the dev config.
export default defineConfig({
  build: { outDir: "dist", assetsDir: "assets", emptyOutDir: true },
  server: { proxy: { "/api": "http://127.0.0.1:8081", "/healthz": "http://127.0.0.1:8081" } },
});
