import { readFileSync, writeFileSync } from "node:fs";
import { expect, type Page } from "@playwright/test";

const control = `http://127.0.0.1:${process.env.SPIELSTAND_STACK_CONTROL_PORT ?? 8470}`;

export type Pc = "Desktop" | "Steam Deck";

async function pcs(): Promise<Record<Pc, { save: string; running: boolean }>> {
  const res = await fetch(`${control}/pcs`);
  return res.json();
}

export async function saveFile(pc: Pc): Promise<string> {
  return (await pcs())[pc].save;
}

export function readSave(path: string): string | null {
  try {
    return readFileSync(path, "utf8");
  } catch {
    return null;
  }
}

/** The player saves: the game writes its file. */
export async function play(pc: Pc, content: string) {
  writeFileSync(await saveFile(pc), content);
}

/** Another program (Stellwerk, a copy by hand) puts `content` on the PC. */
export const copyOnto = play;

export async function expectSave(pc: Pc, content: string, timeout = 20_000) {
  const path = await saveFile(pc);
  await expect.poll(() => readSave(path), { timeout, message: `${pc}'s save` }).toBe(content);
}

export async function switchOff(pc: Pc) {
  await fetch(`${control}/pcs/${encodeURIComponent(pc)}/stop`, { method: "POST" });
}

export async function switchOn(pc: Pc) {
  await fetch(`${control}/pcs/${encodeURIComponent(pc)}/start`, { method: "POST" });
}

export async function login(page: Page, path = "/") {
  await page.goto("/login");
  await page.getByLabel("Passwort").fill("spielstand-stack");
  await page.getByRole("button", { name: "Anmelden" }).click();
  await expect(page.getByRole("navigation", { name: "Hauptnavigation" })).toBeVisible();
  if (path !== "/") await page.goto(path);
}
