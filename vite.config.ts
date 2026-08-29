import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// Must match `build.devUrl` in src-tauri/tauri.conf.json. Deliberately not
// Tauri's default 1420: every Tauri project uses that one, and two of them
// running at once would fight over it.
const PORT = 1751;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // Two documents, because there are two windows. `splash.html` is the launch
  // window and shares nothing with the app but the fonts — it has to paint
  // while the engine is still coming up, so it carries no React and no store.
  build: {
    rollupOptions: {
      input: {
        main: "index.html",
        splash: "splash.html",
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: PORT,
    // Fail instead of quietly moving to the next free port. Without this the
    // window would load whatever else happens to be serving on 1751.
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: PORT + 1,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
