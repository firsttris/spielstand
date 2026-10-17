import type { Activity } from "@/bindings/Activity";
import type { Device } from "@/bindings/Device";
import type { GameDetail } from "@/bindings/GameDetail";
import type { GameSummary } from "@/bindings/GameSummary";
import type { ImportReport } from "@/bindings/ImportReport";
import type { Overview } from "@/bindings/Overview";
import type { PairCode } from "@/bindings/PairCode";
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
  pairCode: () => call<PairCode>("POST", "/api/pair-codes"),
  activity: (limit = 50) => call<Activity[]>("GET", `/api/activity?limit=${limit}`),
  importLudusavi: (source: string) => call<ImportReport>("POST", "/api/import", { source }),
};
