import { defineConfig } from "@playwright/test";
import { resolve } from "node:path";

export default defineConfig({
  testDir: "./tests/ui",
  timeout: 45_000,
  expect: { timeout: 10_000 },
  workers: 1,
  fullyParallel: false,
  reporter: [["list"]],
  outputDir: "output/playwright/test-results",
  use: {
    channel: "chrome",
    headless: true,
    baseURL: "http://127.0.0.1:1421",
    viewport: { width: 1470, height: 923 },
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  webServer: {
    command: "pnpm dev --port 1421",
    url: "http://127.0.0.1:1421",
    reuseExistingServer: false,
    env: { TASKS_DEV_DB: resolve("output/ui-test.sqlite3") },
    timeout: 30_000,
  },
});
