import { paraglideVitePlugin } from "@inlang/paraglide-js";
import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const server = process.env.SPIELSTAND_DEV_SERVER ?? "http://127.0.0.1:8467";

export default defineConfig({
  plugins: [
    paraglideVitePlugin({
      project: "./project.inlang",
      outdir: "./src/paraglide",
      strategy: ["localStorage", "preferredLanguage", "baseLocale"],
    }),
    tanstackRouter({ target: "react", autoCodeSplitting: true }),
    react(),
    tailwindcss(),
  ],
  resolve: { alias: { "@": "/src" } },
  server: {
    proxy: { "/api": { target: server, ws: true }, "/install.sh": server, "/download": server },
  },
  test: { environment: "jsdom", include: ["src/**/*.test.{ts,tsx}"] },
});
