import { defineConfig, devices } from "@playwright/test";

// Runs against the real stack (server, workspace, render) started by `e2e/stack.ts`.
// Needs `cargo build -p board-server -p board-render`, `pnpm build` here and a Playwright Chromium.
export default defineConfig({
  testDir: "e2e",
  globalSetup: "./e2e/stack.ts",
  timeout: 60_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  workers: 1,
  reporter: process.env.CI ? "github" : "list",
  use: {
    ...devices["Desktop Chrome"],
    viewport: { width: 1400, height: 900 },
    trace: "retain-on-failure",
  },
});
