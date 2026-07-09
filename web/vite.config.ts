import { defineConfig } from "vite";

export default defineConfig({
  server: {
    port: 5173,
    // The Rust server owns the socket; Vite serves the app and forwards /ws to
    // it, so `npm run dev` needs no CORS or origin juggling.
    proxy: {
      "/ws": { target: "ws://127.0.0.1:8080", ws: true },
    },
  },
  build: { outDir: "dist", emptyOutDir: true },
  test: {
    environment: "node",
    include: ["tests/**/*.test.ts"],
  },
});
