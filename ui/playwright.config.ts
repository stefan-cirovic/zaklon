import { defineConfig, devices } from "@playwright/test";

// Interface end-to-end tests. They drive the real hub (debug build) in a
// fresh data folder, using the Edge or Chrome already installed on the
// machine, so no browser download is needed.
//
//   cargo build -p zaklon-hub && pnpm build && pnpm e2e
export default defineConfig({
  testDir: "e2e",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 30_000,
  reporter: [["list"]],
  use: {
    baseURL: "http://127.0.0.1:28481",
    channel: process.env.PW_CHANNEL ?? (process.platform === "win32" ? "msedge" : "chrome"),
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    { name: "laptop", use: { viewport: { width: 1280, height: 800 } } },
    { name: "phone", use: { ...devices["Pixel 7"], channel: process.env.PW_CHANNEL ?? (process.platform === "win32" ? "msedge" : "chrome") } },
  ],
  webServer: {
    command: "node e2e/start-hub.mjs",
    url: "http://127.0.0.1:28481/api/status",
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
