import { expect, type Page } from "@playwright/test";

/** The admin password of the demo server (crates/server/examples/demo.rs). */
export const PASSWORD = "spielstand-demo";

export async function login(page: Page, path = "/") {
  await page.goto("/login");
  await page.getByLabel(/Passwort|Password/).fill(PASSWORD);
  await page.getByRole("button", { name: /Anmelden|Sign in/ }).click();
  await expect(page.getByRole("navigation", { name: /Hauptnavigation|Main navigation/ })).toBeVisible();
  if (path !== "/") await page.goto(path);
}
