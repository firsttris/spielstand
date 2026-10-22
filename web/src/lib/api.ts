import type { Activity } from "@/bindings/Activity";
import type { CustomGame } from "@/bindings/CustomGame";
import type { CustomGameRow } from "@/bindings/CustomGameRow";
import type { Device } from "@/bindings/Device";
import type { DeviceSettings } from "@/bindings/DeviceSettings";
import type { GameDetail } from "@/bindings/GameDetail";
import type { GameSummary } from "@/bindings/GameSummary";
import type { ImportReport } from "@/bindings/ImportReport";
import type { Overview } from "@/bindings/Overview";
import type { PairCode } from "@/bindings/PairCode";
import type { SaveMode } from "@/bindings/SaveMode";
import type { ServerSettings } from "@/bindings/ServerSettings";
import type { SessionInfo } from "@/bindings/SessionInfo";
import type { VersionPatch } from "@/bindings/VersionPatch";

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

/** Every call goes through here: session cookie, CSRF header on writes, errors as `ApiError`. */
async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method,
    credentials: "same-origin",
    headers: {
      ...(body === undefined ? {} : { "content-type": "application/json" }),
      ...(method === "GET" ? {} : { "x-spielstand-csrf": "1" }),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text();
    let message = text;
    try {
      message = (JSON.parse(text) as { error?: string }).error ?? text;
    } catch {
      // plain text error
    }
    throw new ApiError(res.status, message);
  }
  const type = res.headers.get("content-type") ?? "";
  return (type.includes("application/json") ? res.json() : undefined) as Promise<T>;
}

const enc = encodeURIComponent;

export const api = {
  session: () => call<SessionInfo>("GET", "/api/session"),
  setup: (password: string) => call<SessionInfo>("POST", "/api/setup", { password }),
  login: (password: string) => call<SessionInfo>("POST", "/api/login", { password }),
  logout: () => call<void>("POST", "/api/logout"),
  overview: () => call<Overview>("GET", "/api/overview"),
  games: () => call<GameSummary[]>("GET", "/api/games"),
  game: (id: string) => call<GameDetail>("GET", `/api/games/${enc(id)}`),
  restore: (game: string, device: string, version: string) =>
    call<void>("POST", `/api/games/${enc(game)}/restore`, { device, version }),
  resolve: (game: string, version: string) =>
    call<void>("POST", `/api/games/${enc(game)}/resolve`, { version }),
  backUp: (game: string, device: string) => call<void>("POST", `/api/games/${enc(game)}/backup`, { device }),
  patchVersion: (id: string, patch: VersionPatch) => call<void>("PATCH", `/api/versions/${enc(id)}`, patch),
  downloadUrl: (id: string) => `/api/versions/${enc(id)}/download`,
  devices: () => call<Device[]>("GET", "/api/devices"),
  removeDevice: (id: string) => call<void>("DELETE", `/api/devices/${enc(id)}`),
  updateDevice: (id: string) => call<void>("POST", `/api/devices/${enc(id)}/update`),
  pairCode: () => call<PairCode>("POST", "/api/pair-codes"),
  activity: (limit = 50) => call<Activity[]>("GET", `/api/activity?limit=${limit}`),
  importLudusavi: (source: string) => call<ImportReport>("POST", "/api/import", { source }),
  settings: () => call<ServerSettings>("GET", "/api/settings"),
  setDefaultMode: (defaultMode: SaveMode) => call<ServerSettings>("PUT", "/api/settings", { defaultMode }),
  /** `null` lets the game follow the default mode again. */
  setGameMode: (game: string, mode: SaveMode | null) =>
    call<void>("PUT", `/api/games/${enc(game)}/mode`, { mode }),
  setPrefix: (game: string, device: string, prefix: string) =>
    call<void>("PUT", `/api/games/${enc(game)}/prefix`, { device, prefix }),
  setGameFolders: (device: string, gameFolders: string[]) =>
    call<DeviceSettings>("PUT", `/api/devices/${enc(device)}/settings`, { gameFolders }),
  dismissAlert: (id: number) => call<void>("POST", `/api/alerts/${id}/dismiss`),
  customGames: () => call<CustomGameRow[]>("GET", "/api/custom-games"),
  /** Adds or changes a custom game for every device (a device's proposal is taken over). */
  putCustomGame: (game: CustomGame) => call<CustomGame>("PUT", "/api/custom-games", game),
  takeOverCustomGame: (name: string) => call<void>("POST", `/api/custom-games/${enc(name)}/take-over`),
  removeCustomGame: (name: string) => call<void>("DELETE", `/api/custom-games/${enc(name)}`),
};
