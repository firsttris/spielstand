import { CheckIcon, HistoryIcon, MonitorIcon, TriangleAlertIcon } from "lucide-react";
import type { GameSummary } from "@/bindings/GameSummary";
import { Badge } from "@/components/ui/badge";
import { m } from "@/paraglide/messages";

/** One badge that says how a game stands across the devices. */
export function GameStatus({ game }: { game: GameSummary }) {
  if (game.conflict)
    return (
      <Badge variant="warning">
        <TriangleAlertIcon /> {m.status_conflict()}
      </Badge>
    );
  if (game.overwritten)
    return (
      <Badge variant="warning">
        <HistoryIcon /> {m.status_overwritten()}
      </Badge>
    );
  if (game.playingOn)
    return (
      <Badge variant="info">
        <MonitorIcon /> {m.status_playing({ device: game.playingOn })}
      </Badge>
    );
  if (!game.head) return <Badge variant="outline">{m.status_imported()}</Badge>;
  if (game.behind.length > 0) return <Badge>{m.status_behind({ devices: game.behind.join(", ") })}</Badge>;
  return (
    <Badge variant="success">
      <CheckIcon /> {m.status_synced()}
    </Badge>
  );
}
