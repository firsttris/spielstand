import { Link } from "@tanstack/react-router";
import { MonitorIcon } from "lucide-react";
import type { GameSummary } from "@/bindings/GameSummary";
import { GameStatus } from "@/components/status";
import { GameTile } from "@/components/ui/misc";
import { relative } from "@/lib/format";
import { gameName } from "@/lib/game-name";
import { m } from "@/paraglide/messages";

const head = "px-3 py-2 text-left text-xs font-medium text-muted-foreground";
const cell = "px-3 py-2.5";

/** Games as a table; the whole row is clickable through a stretched link in the first cell. */
export function GameTable({ games }: { games: GameSummary[] }) {
  if (games.length === 0)
    return <p className="px-3 py-6 text-center text-sm text-muted-foreground">{m.games_empty()}</p>;
  return (
    <div className="overflow-x-auto">
      <table className="w-full min-w-[640px] border-collapse" aria-label={m.nav_games()}>
        <thead>
          <tr className="border-b">
            <th className={head}>{m.col_game()}</th>
            <th className={head}>{m.col_last_device()}</th>
            <th className={head}>{m.col_last_version()}</th>
            <th className={head}>{m.col_status()}</th>
          </tr>
        </thead>
        <tbody>
          {games.map((g) => (
            <tr key={g.id} className="relative border-b border-border/50 last:border-0 hover:bg-accent/50">
              <td className={cell}>
                <span className="flex min-w-0 items-center gap-3">
                  <GameTile title={gameName(g)} />
                  <span className="flex min-w-0 flex-col">
                    <Link
                      to="/games/$gameId"
                      params={{ gameId: g.id }}
                      className="truncate font-medium after:absolute after:inset-0 focus-visible:outline-none"
                    >
                      {gameName(g)}
                    </Link>
                    <span className="text-xs text-muted-foreground">
                      {m.versions_count({ n: g.versions })}
                    </span>
                  </span>
                </span>
              </td>
              <td className={`${cell} text-sm`}>
                <span className="flex items-center gap-1.5">
                  <MonitorIcon className="size-3.5 text-muted-foreground" /> {g.headDeviceName ?? "–"}
                </span>
              </td>
              <td className={`${cell} text-sm`}>{g.head ? relative(g.head.createdAt) : "–"}</td>
              <td className={cell}>
                <GameStatus game={g} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
