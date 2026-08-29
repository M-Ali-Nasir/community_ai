import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const COORDINATOR = process.env.COORDINATOR_URL ?? "http://localhost:8787";

export default defineConfig({
  base: "./",
  plugins: [react()],
  server: {
    port: 5173,
    // In dev the PWA is served by Vite, so API and WebSocket calls are proxied
    // to the coordinator. In production the coordinator serves the built app
    // itself and both live on the same origin.
    proxy: {
      "/api": { target: COORDINATOR, changeOrigin: true },
      "/ws": { target: COORDINATOR, ws: true, changeOrigin: true },
    },
  },
  build: {
    outDir: "dist",
    sourcemap: true,
    chunkSizeWarningLimit: 2500,
  },
  worker: {
    format: "es",
  },
  optimizeDeps: {
    exclude: ["@mlc-ai/web-llm"],
  },
});
