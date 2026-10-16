import { defineConfig } from "vite";

// The Rust API runs on :8080. We proxy /api through Vite so the browser makes
// same-origin requests in dev (no CORS dance, keys stay server-side anyway).
export default defineConfig({
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: "http://127.0.0.1:8080",
        changeOrigin: true,
      },
    },
  },
});
