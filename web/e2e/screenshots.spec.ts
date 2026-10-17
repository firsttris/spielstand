import { expect, test } from "@playwright/test";
import { login } from "./helpers";

// Pictures for the README and the docs. Started by hand (pnpm screenshots) or by the
// Screenshots workflow; the files are committed.
const dir = "../docs/screenshots";

test.beforeEach(async ({ page }) => {
  await login(page);
  // The demo copies its versions to the mirror right after the start.
  await expect(page.getByText(/mirror current/).first()).toBeVisible({ timeout: 30_000 });
});

test("overview", async ({ page }) => {
  await expect(page.getByRole("table", { name: "Games" }).getByRole("link")).toHaveCount(6);
  await page.screenshot({ path: `${dir}/overview.png` });
});

test("game with two states", async ({ page }) => {
  await page.goto("/games/Stardew%20Valley");
  await expect(page.getByText("Played on two PCs")).toBeVisible();
  await page.screenshot({ path: `${dir}/conflict.png`, fullPage: true });
});

test("versions of a game", async ({ page }) => {
  await page.goto("/games/ELDEN%20RING");
  await expect(page.getByRole("button", { name: "Load this version" })).toBeVisible();
  await page.screenshot({ path: `${dir}/versions.png`, fullPage: true });
});

test("where each device keeps the save", async ({ page }) => {
  await page.goto("/games/Stardew%20Valley");
  await expect(page.getByRole("heading", { name: "On your devices" })).toBeVisible();
  await page.getByRole("heading", { name: "On your devices" }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${dir}/locations.png`, fullPage: true });
});

test("an older save put back", async ({ page }) => {
  await page.goto("/games/Baldur's%20Gate%203");
  await expect(page.getByRole("alert", { name: "Older save put back" })).toBeVisible();
  await page.screenshot({ path: `${dir}/overwritten.png` });
});

test("devices", async ({ page }) => {
  await page.goto("/devices");
  await expect(page.getByRole("region", { name: "Steam Deck" })).toBeVisible();
  await page.screenshot({ path: `${dir}/devices.png`, fullPage: true });
});

test("pairing", async ({ page }) => {
  await page.goto("/devices");
  await page.getByRole("button", { name: "Add device" }).click();
  await expect(page.getByRole("dialog").locator("code")).toContainText("install.sh");
  await page.screenshot({ path: `${dir}/pairing.png` });
});

test("overview in dark mode", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(page.getByRole("table", { name: "Games" }).getByRole("link")).toHaveCount(6);
  await page.screenshot({ path: `${dir}/overview-dark.png` });
});
