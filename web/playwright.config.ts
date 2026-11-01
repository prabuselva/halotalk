import { defineConfig } from "@playwright/test";

const BASE = process.env.E2E_BASE ?? "http://127.0.0.1:8420";

export default defineConfig({
  testDir: "./e2e",
  timeout: 90_000,
  reporter: [["list"]],
  use: {
    baseURL: BASE,
    permissions: [],
  },
});
