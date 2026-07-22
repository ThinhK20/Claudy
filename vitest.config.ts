import { defineConfig } from "vitest/config";
import { fileURLToPath } from "node:url";

// Test config kept separate from vite.config.ts so the Tauri-tuned dev server
// settings don't apply to the test run. Reuses the `@` -> ./src alias.
export default defineConfig({
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
  },
});
