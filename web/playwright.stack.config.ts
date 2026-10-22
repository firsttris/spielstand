import { defineConfig, devices } from "@playwright/test";

// The whole chain: a real server and two real agents (Desktop, Steam Deck), started by the
// `stack` example (crates/server/examples/stack). Tests write and read the PCs' save files
// directly and act in the browser. Build first: `cargo build -p spielstand-agent` and
// `pnpm build`.
const port = Number(process.env.SPIELSTAND_STACK_PORT ?? 8469);
const control = Number(process.env.SPIELSTAND_STACK_CONTROL_PORT ?? 8470);
const stack = process.env.SPIELSTAND_STACK_BIN ?? "cargo run -q -p spielstand-server --example stack";
const executablePath = process.env.PLAYWRIGHT_CHROMIUM || undefined;

export default defineConfig({
  testDir: "./e2e-stack",
  workers: 1,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  timeout: 60_000,
  reporter: process.env.CI
    ? [["github"], ["html", { open: "never", outputFolder: "playwright-report-stack" }]]
    : "list",
  use: {
    ...devices["Desktop Chrome"],
    baseURL: `http://127.0.0.1:${port}`,
    locale: "de-DE",
    trace: "retain-on-failure",
    launchOptions: { executablePath },
  },
  webServer: {
    command: stack,
    cwd: "..",
    // The control port opens last, once both PCs are paired and running; the server is up long
    // before that.
    url: `http://127.0.0.1:${control}/pcs`,
    env: {
      SPIELSTAND_LISTEN: `127.0.0.1:${port}`,
      SPIELSTAND_STACK_CONTROL: `127.0.0.1:${control}`,
      RUST_LOG: "warn",
    },
    timeout: 900_000,
    reuseExistingServer: false,
    stdout: "pipe",
  },
});
