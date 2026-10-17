import type { GameLocation } from "@/bindings/GameLocation";

/** How one device stands for a game. */
export type Standing = "current" | "behind" | "diverged" | "none" | "unknown";

/** What the web UI asks of the person for one device, if anything. */
export type Need =
  | { kind: "nothing" }
  /** Another program still syncs the saves: Spielstand only backs up there for now. */
  | { kind: "foreignSync"; tool: string; folder: string }
  | { kind: "choosePrefix"; candidates: string[] }
  | { kind: "failed"; message: string };

export type LocationView = {
  standing: Standing;
  need: Need;
  /** The device follows "Nur sichern" although the game is set to sync. */
  backupOnly: boolean;
};

/** Pure: everything the device row shows, from what the server knows about the device.
 * `branches` are the game's versions kept as a second state next to the current one. */
export function viewLocation(
  location: GameLocation,
  branches: ReadonlySet<string> = new Set(),
): LocationView {
  const report = location.report;
  const standing: Standing = location.isCurrent
    ? "current"
    : location.version && branches.has(location.version)
      ? "diverged"
      : location.version
        ? "behind"
        : report?.location
          ? "unknown"
          : "none";
  const problem = report?.problem;
  const foreign = report?.foreignSync;
  const need: Need =
    problem?.kind === "noPrefix"
      ? { kind: "choosePrefix", candidates: [] }
      : problem?.kind === "severalPrefixes"
        ? { kind: "choosePrefix", candidates: problem.candidates }
        : problem?.kind === "failed"
          ? { kind: "failed", message: problem.message }
          : foreign && !foreign.excluded
            ? { kind: "foreignSync", tool: foreign.tool, folder: foreign.folder }
            : { kind: "nothing" };
  return { standing, need, backupOnly: report?.effectiveMode === "backup" };
}
