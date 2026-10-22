import { expect, test } from "@playwright/test";
import { login } from "./helpers";

const devices = (page: import("@playwright/test").Page) =>
  page.getByRole("list").filter({ has: page.getByRole("listitem", { name: "Wohnzimmer-PC" }) });

test("the game page says where each device keeps the save", async ({ page }) => {
  await login(page, "/games/Hades%20II");
  await expect(page.getByRole("heading", { name: "Auf deinen Geräten" })).toBeVisible();
  const living = devices(page).getByRole("listitem", { name: "Wohnzimmer-PC" });
  await expect(living.getByText("Prefixr")).toBeVisible();
  await expect(living.getByText("Aktuell")).toBeVisible();
  await expect(living.getByText("/home/tristan/Games/Hades II/prefix", { exact: false })).toBeVisible();
  const deck = devices(page).getByRole("listitem", { name: "Steam Deck" });
  await expect(deck.getByText("/run/media/deck/SD/Games/Hades II/prefix", { exact: false })).toBeVisible();
});

test("a save another program still syncs is explained", async ({ page }) => {
  await login(page, "/games/Stardew%20Valley");
  const office = devices(page).getByRole("listitem", { name: "Arbeitszimmer" });
  await expect(office.getByText("Sichert nur")).toBeVisible();
  await expect(office.getByText(/Stellwerk synchronisiert diesen Spielstand noch mit/)).toBeVisible();
});

test("several possible prefixes are a question with a choice", async ({ page }) => {
  await login(page, "/games/Hollow%20Knight");
  const deck = devices(page).getByRole("listitem", { name: "Steam Deck" });
  await expect(
    deck.getByText("Mehrere Prefixe könnten zu diesem Spiel gehören. Welcher ist es?"),
  ).toBeVisible();
  await deck.getByRole("combobox").click();
  await page.getByRole("option", { name: "/run/media/deck/SD/Games/Hollow Knight/pfx" }).click();
  const saved = page.waitForResponse((r) => r.url().includes("/prefix") && r.request().method() === "PUT");
  await deck.getByRole("button", { name: "Prefix übernehmen" }).click();
  expect((await saved).ok()).toBeTruthy();
});

test("a Switch game shows its name and what Ryujinx needs first", async ({ page }) => {
  await login(page, "/games");
  await page.getByRole("link", { name: "The Legend of Zelda: Tears of the Kingdom" }).click();
  await expect(
    page.getByRole("heading", { name: "The Legend of Zelda: Tears of the Kingdom", level: 1 }),
  ).toBeVisible();
  const living = devices(page).getByRole("listitem", { name: "Wohnzimmer-PC" });
  await expect(living.getByText("Eden", { exact: true })).toBeVisible();
  const deck = devices(page).getByRole("listitem", { name: "Steam Deck" });
  await expect(deck.getByText(/Starte das Spiel einmal in Ryujinx/)).toBeVisible();
});

test("a game's mode is chosen on its page", async ({ page }) => {
  await login(page, "/games/Hollow%20Knight");
  const mode = page.getByRole("combobox", { name: "Modus" });
  await expect(mode).toHaveText("Nur sichern");
  await mode.click();
  await page.getByRole("option", { name: "Standard: Synchronisieren" }).click();
  await expect(mode).toHaveText("Standard: Synchronisieren");
  await page.reload();
  await expect(page.getByRole("combobox", { name: "Modus" })).toHaveText("Standard: Synchronisieren");
});

test("the default mode is set in the settings", async ({ page }) => {
  await login(page, "/settings");
  const mode = page.getByRole("combobox", { name: "Standardmodus" });
  await expect(mode).toHaveText("Synchronisieren");
  await mode.click();
  // Reload only once the server has the new mode, or the page may still show the old one.
  const saved = page.waitForResponse(
    (r) => r.url().endsWith("/api/settings") && r.request().method() === "PUT",
  );
  await page.getByRole("option", { name: "Nur sichern" }).click();
  await saved;
  await page.reload();
  await expect(page.getByRole("combobox", { name: "Standardmodus" })).toHaveText("Nur sichern");
  await page.getByRole("combobox", { name: "Standardmodus" }).click();
  await page.getByRole("option", { name: "Synchronisieren" }).click();
  await expect(page.getByRole("combobox", { name: "Standardmodus" })).toHaveText("Synchronisieren");
});

test("game folders are set per device", async ({ page }) => {
  await login(page, "/devices");
  const living = page.getByRole("region", { name: "Wohnzimmer-PC" });
  await expect(living.getByText("Stellwerk")).toBeVisible();
  const folders = living.getByLabel("Spiele-Ordner");
  await expect(folders).toHaveValue("~/Games");
  await folders.fill("~/Games\n/mnt/spiele");
  await living.getByRole("button", { name: "Ordner speichern" }).click();
  await expect(living.getByText("Gespeichert. Das Gerät sucht jetzt dort.")).toBeVisible();
  await page.reload();
  await expect(page.getByRole("region", { name: "Wohnzimmer-PC" }).getByLabel("Spiele-Ordner")).toHaveValue(
    "~/Games\n/mnt/spiele",
  );
});

// Changes what the overview shows, so it runs last.
test("an overwritten save is shown, can be brought back and dismissed", async ({ page }) => {
  await login(page);
  await expect(page.getByText("1 Spielstand/Spielstände durch ältere ersetzt")).toBeVisible();
  await page.getByRole("link", { name: "Baldur's Gate 3" }).first().click();
  const alert = page.getByRole("alert", { name: "Älterer Spielstand eingespielt" });
  await expect(
    alert.getByText(/Auf Arbeitszimmer wurde der Stand vom .* durch einen älteren ersetzt/),
  ).toBeVisible();
  // The Arbeitszimmer is switched off: the server says so instead of pretending.
  await alert.getByRole("button", { name: "Neueren Stand auf Arbeitszimmer zurückholen" }).click();
  await expect(alert.getByText("Das Gerät ist nicht verbunden")).toBeVisible();
  await alert.getByRole("button", { name: "Ausblenden" }).click();
  await expect(alert).toBeHidden();
  await page.goto("/");
  await expect(page.getByText("1 Spielstand/Spielstände durch ältere ersetzt")).toBeHidden();
});
