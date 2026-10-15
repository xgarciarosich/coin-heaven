import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

const src = (p: string) => path.resolve(import.meta.dirname, p);

// Tauri drives this dev server; the fixed port and strictPort are required so
// the Rust side knows where to point the webview.
export default defineConfig(({ command }) => ({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": src("./src"),
      // `ipc.ts` falls back to fixtures when it runs outside Tauri under
      // `pnpm dev`. A release build aliases them to a throwing stub instead:
      // a dynamic import survives tree-shaking, and a finance app has no
      // business shipping invented figures inside its binary.
      "@fixtures":
        command === "build" ? src("./src/lib/mock.stub.ts") : src("./src/lib/mock.ts"),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: { target: "es2022", sourcemap: false },
}));
