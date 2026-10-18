import { expect, test } from "@playwright/test";
import { login } from "./helpers";

test("a device's proposed custom game is only backed up there until it is taken over", async ({ page }) => {
  await login(page, "/games/Pixel%20Quest");
  const deck = page.getByRole("listitem", { name: "Steam Deck" });
  await expect(deck.getByText("Eigenes Spiel", { exact: true })).toBeVisible();
  await expect(deck.getByText("Sichert nur")).toBeVisible();
  await expect(deck.getByText(/von diesem PC vorgeschlagen/)).toBeVisible();
  await deck.getByRole("link", { name: "Zu den eigenen Spielen" }).click();

  const list = page.getByRole("list", { name: "Eigene Spiele" });
  const pixel = list.getByRole("listitem", { name: "Pixel Quest" });
  await expect(pixel.getByText("Vorgeschlagen von Steam Deck, gilt nur dort")).toBeVisible();
  await expect(pixel.getByText("<xdgData>/PixelQuest/saves")).toBeVisible();
  const hades = list.getByRole("listitem", { name: "Hades II" });
  await expect(hades.getByText("Ergänzt ein bekanntes Spiel")).toBeVisible();

  await pixel.getByRole("button", { name: "Für alle PCs übernehmen" }).click();
  await expect(pixel.getByText("Alle PCs")).toBeVisible();
});

test("a custom game is added for every PC, and a path that catches too much is refused", async ({ page }) => {
  await login(page, "/settings");
  await page.getByLabel("Name des Spiels").fill("Tiny Farm");
  await page.getByLabel("Pfade").fill("<home>/*");
  await page.getByRole("button", { name: "Für alle PCs speichern" }).click();
  await expect(page.getByText(/the first folder after the placeholder/)).toBeVisible();

  await page.getByLabel("Pfade").fill("<xdgData>/TinyFarm/saves\n<winAppData>/TinyFarm");
  await page.getByRole("button", { name: "Für alle PCs speichern" }).click();
  const farm = page.getByRole("list", { name: "Eigene Spiele" }).getByRole("listitem", { name: "Tiny Farm" });
  await expect(farm.getByText("Alle PCs")).toBeVisible();
  await expect(farm.getByText("<winAppData>/TinyFarm")).toBeVisible();
  await farm.getByRole("button", { name: "Entfernen" }).click();
  await expect(farm).toBeHidden();
});
