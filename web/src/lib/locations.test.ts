import { describe, expect, it } from "vitest";
import type { GameLocation } from "@/bindings/GameLocation";
import type { GameReport } from "@/bindings/GameReport";
import { viewLocation } from "./locations";

const report = (r: Partial<GameReport> = {}): GameReport => ({
  game: "Hades",
  location: "/home/deck/Games/Hades/prefix/drive_c/users/steamuser",
  prefix: "/home/deck/Games/Hades/prefix",
  source: "prefixr",
  effectiveMode: "sync",
  foreignSync: null,
  problem: null,
  ...r,
});

const at = (l: Partial<GameLocation> = {}): GameLocation => ({
  deviceId: "d",
  deviceName: "Steam Deck",
  online: true,
  version: "v1",
  isCurrent: true,
  report: report(),
  ...l,
});

describe("viewLocation", () => {
  it("says where a device stands", () => {
    expect(viewLocation(at()).standing).toBe("current");
    expect(viewLocation(at({ isCurrent: false })).standing).toBe("behind");
    expect(viewLocation(at({ isCurrent: false }), new Set(["v1"])).standing).toBe("diverged");
    expect(viewLocation(at({ isCurrent: false, version: null })).standing).toBe("unknown");
    expect(viewLocation(at({ isCurrent: false, version: null, report: null })).standing).toBe("none");
  });

  it("asks for a prefix when none or several were found", () => {
    expect(viewLocation(at({ report: report({ problem: { kind: "noPrefix" } }) })).need).toEqual({
      kind: "choosePrefix",
      candidates: [],
    });
    const several = report({ problem: { kind: "severalPrefixes", candidates: ["/a", "/b"] } });
    expect(viewLocation(at({ report: several })).need).toEqual({
      kind: "choosePrefix",
      candidates: ["/a", "/b"],
    });
  });

  it("asks which emulator holds a Switch game, or to start it once in Ryujinx", () => {
    const several = report({ problem: { kind: "severalEmulators", candidates: ["/eden/X", "/ryu/0"] } });
    expect(viewLocation(at({ report: several })).need).toEqual({
      kind: "chooseEmulator",
      candidates: ["/eden/X", "/ryu/0"],
    });
    const start = report({ problem: { kind: "startInEmulator", emulator: "Ryujinx" } });
    expect(viewLocation(at({ report: start })).need).toEqual({
      kind: "startInEmulator",
      emulator: "Ryujinx",
    });
  });

  it("explains why a device only backs up while another program syncs the saves", () => {
    const view = viewLocation(
      at({
        report: report({
          effectiveMode: "backup",
          foreignSync: { tool: "stellwerk", folder: "/home/deck/Games", excluded: false },
        }),
      }),
    );
    expect(view.backupOnly).toBe(true);
    expect(view.need).toEqual({ kind: "foreignSync", tool: "stellwerk", folder: "/home/deck/Games" });
    const excluded = report({ foreignSync: { tool: "stellwerk", folder: "/g", excluded: true } });
    expect(viewLocation(at({ report: excluded })).need).toEqual({ kind: "nothing" });
  });

  it("puts a failed load before everything else but a missing prefix", () => {
    const failed = report({
      problem: { kind: "failed", message: "disk full" },
      foreignSync: { tool: "syncthing", folder: "/g", excluded: false },
    });
    expect(viewLocation(at({ report: failed })).need).toEqual({ kind: "failed", message: "disk full" });
  });
});
