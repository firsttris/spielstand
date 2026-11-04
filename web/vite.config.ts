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
      // Paraglide 2's default: one module per locale in the dev server and tests (a few requests),
      // one per message in production builds (tree shaking). v3 would serve virtual modules only Vite
      // can resolve.
      outputStructure: process.env.NODE_ENV === "production" ? "message-modules" : "locale-modules",
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
