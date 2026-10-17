import { expect, test } from "@playwright/test";
import { copyOnto, expectSave, login, play, saveFile, switchOff, switchOn } from "./stack";

// One story on two real PCs, in order: each step builds on the state the one before left.
test.describe.configure({ mode: "serial" });

test("the deck gets the desktop's save on its own, into its own prefix", async ({ page }) => {
  await expectSave("Steam Deck", "desktop: first run");
  expect(await saveFile("Steam Deck")).toContain("/sd/Games/Hades/prefix/drive_c/users/steamuser/");
  await login(page, "/games/Hades");
  const deck = page.getByRole("listitem", { name: "Steam Deck" });
  await expect(deck.getByText("Prefixr")).toBeVisible();
  await expect(deck.getByText("Aktuell")).toBeVisible();
  await expect(deck.getByText("/sd/Games/Hades/prefix", { exact: false })).toBeVisible();
});

test("a change while playing reaches the other PC within seconds", async () => {
  await play("Desktop", "desktop: boss beaten");
  await expectSave("Steam Deck", "desktop: boss beaten");
  await play("Steam Deck", "deck: new weapon");
  await expectSave("Desktop", "deck: new weapon");
});

test("an older version is loaded from the web UI and every PC continues from it", async ({ page }) => {
  await login(page, "/games/Hades");
  // The oldest version: the desktop's first run.
  await page.getByRole("list", { name: "Versionen" }).getByRole("button").last().click();
  await page.getByRole("combobox", { name: "Ziel-PC" }).click();
  await page.getByRole("option", { name: "auf Steam Deck" }).click();
  await page.getByRole("button", { name: "Diese Version laden" }).click();
  await expect(page.getByText("An Steam Deck geschickt", { exact: false })).toBeVisible();
  await expectSave("Steam Deck", "desktop: first run");
  await expectSave("Desktop", "desktop: first run");
});

test("played on both while one was off: two states, the web UI decides", async ({ page }) => {
  await play("Steam Deck", "deck: before the trip");
  await expectSave("Desktop", "deck: before the trip");
  await switchOff("Steam Deck");
  await play("Steam Deck", "deck: played offline");
  await play("Desktop", "desktop: played meanwhile");
  await login(page, "/games/Hades");
  // The desktop's change is the new head; the deck comes back with its own.
  await switchOn("Steam Deck");
  await expect(page.getByText("Auf zwei PCs gespielt")).toBeVisible({ timeout: 30_000 });
  // Both states are kept: nothing was written over the deck's offline play.
  await expectSave("Steam Deck", "deck: played offline", 2_000);
  await page
    .getByRole("region", { name: "Desktop" })
    .getByRole("button", { name: "Mit diesem Stand weiterspielen" })
    .click();
  await expectSave("Steam Deck", "desktop: played meanwhile");
  await expect(page.getByText("Auf zwei PCs gespielt")).toBeHidden();
});

test("backup only: an older copy from another program is reported and undone", async ({ page }) => {
  await login(page, "/games/Hades");
  await page.getByRole("combobox", { name: "Modus" }).click();
  await page.getByRole("option", { name: "Nur sichern" }).click();
  await expect(page.getByRole("combobox", { name: "Modus" })).toHaveText("Nur sichern");

  await play("Steam Deck", "deck: final chapter");
  await page.waitForTimeout(3_000);
  // In backup mode Spielstand does not move it: the desktop keeps its state.
  await expectSave("Desktop", "desktop: played meanwhile", 2_000);
  // Another program now copies the desktop's older save over the deck's newer one.
  await copyOnto("Steam Deck", "desktop: played meanwhile");
  const alert = page.getByRole("alert", { name: "Älterer Spielstand eingespielt" });
  await expect(alert).toBeVisible({ timeout: 30_000 });
  await expectSave("Steam Deck", "desktop: played meanwhile", 2_000);
  await alert.getByRole("button", { name: "Neueren Stand auf Steam Deck zurückholen" }).click();
  await expectSave("Steam Deck", "deck: final chapter");
  await expect(alert).toBeHidden({ timeout: 15_000 });
});
