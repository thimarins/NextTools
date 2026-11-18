import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": path.resolve(import.meta.dirname, "src") } },
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2022", sourcemap: false, chunkSizeWarningLimit: 1500 },
});
