import { defineConfig, devices } from "@playwright/test";

// The end-to-end tests run against the real server, filled with demo data by the `demo` example
// (crates/server/examples/demo.rs). It serves the built web UI, so run `pnpm build` first.
const port = Number(process.env.SPIELSTAND_E2E_PORT ?? 8468);
const baseURL = `http://127.0.0.1:${port}`;
// CI builds the demo once and hands the binary to the Playwright container.
const demo = process.env.SPIELSTAND_DEMO_BIN ?? "cargo run -q -p spielstand-server --example demo";
// A local Chromium for machines where the Playwright download is not possible.
const executablePath = process.env.PLAYWRIGHT_CHROMIUM || undefined;

export default defineConfig({
  testDir: "./e2e",
  // One server with one set of data: tests that change it run one after the other.
  workers: 1,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["github"], ["html", { open: "never" }]] : "list",
  use: { baseURL, trace: "retain-on-failure", launchOptions: { executablePath } },
  webServer: {
    command: demo,
    cwd: "..",
    url: `${baseURL}/healthz`,
    env: { SPIELSTAND_LISTEN: `127.0.0.1:${port}`, RUST_LOG: "warn" },
    // The first run compiles the server.
    timeout: 900_000,
    reuseExistingServer: false,
    stdout: "pipe",
  },
  projects: [
    {
      name: "e2e",
      testIgnore: /screenshots\.spec\.ts/,
      use: { ...devices["Desktop Chrome"], locale: "de-DE" },
    },
    {
      name: "screenshots",
      testMatch: /screenshots\.spec\.ts/,
      use: {
        ...devices["Desktop Chrome"],
        locale: "en-US",
        viewport: { width: 1440, height: 900 },
        deviceScaleFactor: 1,
      },
    },
  ],
});
