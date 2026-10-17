import { expect, test } from "@playwright/test";
import { login } from "./helpers";

test("a wrong password is refused", async ({ page }) => {
  await page.goto("/");
  await expect(page).toHaveURL(/\/login$/);
  await page.getByLabel("Passwort").fill("falsch-falsch");
  await page.getByRole("button", { name: "Anmelden" }).click();
  await expect(page.getByRole("alert")).toHaveText("Das Passwort stimmt nicht.");
});

test("the overview shows the games and the open conflict", async ({ page }) => {
  await login(page);
  await expect(page.getByRole("heading", { name: "Übersicht" })).toBeVisible();
  await expect(page.getByText("1 Spiel(e) wurden auf zwei PCs gespielt")).toBeVisible();
  const games = page.getByRole("table", { name: "Spiele" });
  for (const game of ["ELDEN RING", "Hades II", "Stardew Valley", "Hollow Knight", "Baldur's Gate 3"]) {
    await expect(games.getByRole("link", { name: game, exact: true })).toBeVisible();
  }
  await expect(games.getByText("Läuft auf Steam Deck")).toBeVisible();
});

test("searching narrows the game list", async ({ page }) => {
  await login(page, "/games");
  await page.getByPlaceholder("Spiel suchen …").fill("hollow");
  const games = page.getByRole("table", { name: "Spiele" });
  await expect(games.getByRole("link")).toHaveCount(1);
  await expect(games.getByRole("link", { name: "Hollow Knight" })).toBeVisible();
});

test("devices show who is online and what is played", async ({ page }) => {
  await login(page, "/devices");
  const deck = page.getByRole("region", { name: "Steam Deck" });
  await expect(deck.getByText("Spielt", { exact: true })).toBeVisible();
  await expect(deck.getByText(/Hades II · seit/)).toBeVisible();
  await expect(page.getByRole("region", { name: "Wohnzimmer-PC" }).getByText("Online")).toBeVisible();
  await expect(page.getByRole("region", { name: "Arbeitszimmer" }).getByText("Offline")).toBeVisible();
});

test("pairing shows a one-line install command", async ({ page, request }) => {
  await login(page, "/devices");
  await page.getByRole("button", { name: "Gerät hinzufügen" }).click();
  const dialog = page.getByRole("dialog", { name: "Neues Gerät koppeln" });
  const command = dialog.locator("code");
  await expect(command).toContainText(/curl -fsSL http:\/\/127\.0\.0\.1:\d+\/install\.sh \| sh -s -- \S+/);
  await expect(dialog.getByText("Warte auf das neue Gerät …")).toBeVisible();

  const script = await request.get("/install.sh");
  expect(script.ok()).toBeTruthy();
  expect(await script.text()).toContain("spielstand-agent");
});

test("an older version can be loaded on a connected PC", async ({ page }) => {
  await login(page, "/games/ELDEN%20RING");
  await expect(page.getByRole("heading", { name: "ELDEN RING", level: 1 })).toBeVisible();
  await expect(page.getByText("4 Version(en)", { exact: false })).toBeVisible();
  await expect(page.getByText(/Alle PCs spielen danach mit diesem Stand weiter/)).toBeVisible();
  await page.getByRole("button", { name: "Diese Version laden" }).click();
  await expect(page.getByText(/An .+ geschickt/)).toBeVisible();
});

test("a version can be named and kept", async ({ page }) => {
  await login(page, "/games/Hollow%20Knight");
  await page.getByLabel("Name dieser Version").fill("Vor dem Endboss");
  await page.getByRole("button", { name: "Speichern" }).click();
  await page.getByRole("button", { name: "Behalten" }).click();
  await expect(page.getByRole("button", { name: "Nicht mehr behalten" })).toBeVisible();
  await page.reload();
  await expect(page.getByText("Vor dem Endboss").first()).toBeVisible();
});

test("the language can be switched", async ({ page }) => {
  await login(page, "/settings");
  await page.getByRole("combobox", { name: "Sprache" }).click();
  await page.getByRole("option", { name: "English" }).click();
  await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
  await page.getByRole("combobox", { name: "Language" }).click();
  await page.getByRole("option", { name: "Deutsch" }).click();
  await expect(page.getByRole("heading", { name: "Einstellungen" })).toBeVisible();
});

// Changes the data the other tests look at, so it runs last.
test("choosing a side ends the conflict", async ({ page }) => {
  await login(page, "/games/Stardew%20Valley");
  await expect(page.getByText("Auf zwei PCs gespielt")).toBeVisible();
  await page.getByRole("button", { name: "Mit diesem Stand weiterspielen" }).first().click();
  await expect(page.getByText("Auf zwei PCs gespielt")).toBeHidden();
  await page.goto("/");
  await expect(page.getByText("Alles synchron")).toBeVisible();
});
