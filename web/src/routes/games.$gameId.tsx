import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { useState } from "react";
import { OverwriteAlerts } from "@/components/game/alerts";
import { ConflictChooser } from "@/components/game/conflict";
import { Locations } from "@/components/game/locations";
import { ModeSelect } from "@/components/game/mode-select";
import { VersionList } from "@/components/game/version-list";
import { VersionPanel } from "@/components/game/version-panel";
import { Badge } from "@/components/ui/badge";
import { GameTile, Skeleton } from "@/components/ui/misc";
import { api } from "@/lib/api";
import { bytes } from "@/lib/format";
import { m } from "@/paraglide/messages";

export const Route = createFileRoute("/games/$gameId")({ component: Game });

function Game() {
  const { gameId } = Route.useParams();
  const game = useQuery({
    queryKey: ["game", gameId],
    queryFn: () => api.game(gameId),
    refetchInterval: 5_000,
  });
  const [selected, setSelected] = useState<string>();

  if (game.isError) return <p className="text-muted-foreground">{m.game_missing()}</p>;
  const g = game.data;
  if (!g) return <Skeleton className="h-96" />;

  const current =
    g.versions.find((v) => v.meta.id === selected) ?? g.versions.find((v) => !v.isHead) ?? g.versions[0];
  const total = g.versions.reduce((sum, v) => sum + v.meta.blobSize, 0);
  const head = g.versions.find((v) => v.isHead);
  const behind = head ? head.onDevices.length : 0;

  return (
    <>
      <nav
        aria-label={m.breadcrumb()}
        className="flex items-center gap-1.5 text-[13px] text-muted-foreground"
      >
        <Link to="/games" className="hover:text-foreground">
          {m.nav_games()}
        </Link>
        <span aria-hidden="true">/</span>
        <span className="text-foreground">{g.id}</span>
      </nav>

      <header className="flex flex-wrap items-center gap-4">
        <GameTile title={g.id} size={56} />
        <div className="flex min-w-72 flex-1 flex-col gap-1.5">
          <h1 className="text-2xl font-semibold tracking-tight">{g.id}</h1>
          <div className="flex flex-wrap items-center gap-1.5">
            {g.conflict ? (
              <Badge variant="warning">{m.status_conflict()}</Badge>
            ) : g.alerts.length > 0 ? (
              <Badge variant="warning">{m.status_overwritten()}</Badge>
            ) : g.playingOn ? (
              <Badge variant="info">{m.status_playing({ device: g.playingOn })}</Badge>
            ) : head ? (
              <Badge variant="success">{m.head_on({ n: behind })}</Badge>
            ) : (
              <Badge variant="outline">{m.status_imported()}</Badge>
            )}
            <span className="text-[13px] text-muted-foreground">
              {m.versions_count({ n: g.versions.length })} · {bytes(total)}
            </span>
          </div>
        </div>
      </header>

      <OverwriteAlerts alerts={g.alerts} />
      {g.conflict && <ConflictChooser game={g} />}

      <div className="flex flex-wrap items-start gap-5">
        <VersionList versions={g.versions} selected={current?.meta.id} onSelect={setSelected} />
        {current && (
          <VersionPanel
            key={current.meta.id}
            game={g}
            row={current}
            // Loading a version can make it the head; the panel stays on it rather than jumping.
            onRestore={() => setSelected(current.meta.id)}
          />
        )}
      </div>

      <Locations game={g} />
      <ModeSelect game={g} />
    </>
  );
}
