import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { CheckIcon, MonitorIcon, TriangleAlertIcon } from "lucide-react";
import { GameTable } from "@/components/game-table";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { PageHeader, Skeleton } from "@/components/ui/misc";
import { api } from "@/lib/api";
import { bytes, relative } from "@/lib/format";
import { m } from "@/paraglide/messages";

export const Route = createFileRoute("/")({ component: Overview });

function Tile({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-0.5 rounded-lg border px-3.5 py-3">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="font-medium">{value}</span>
    </div>
  );
}

function Overview() {
  const overview = useQuery({ queryKey: ["overview"], queryFn: api.overview, refetchInterval: 5_000 });
  const games = useQuery({ queryKey: ["games"], queryFn: api.games, refetchInterval: 5_000 });
  const o = overview.data;
  const conflicts = (games.data ?? []).filter((g) => g.conflict);

  return (
    <>
      <PageHeader title={m.nav_overview()} description={m.overview_subtitle()}>
        <Button variant="outline" asChild>
          <Link to="/devices">
            <MonitorIcon /> {m.devices_add()}
          </Link>
        </Button>
      </PageHeader>

      {!o ? (
        <Skeleton className="h-40" />
      ) : (
        <Card className="flex flex-col gap-5 p-6">
          <div className="flex flex-wrap items-center gap-4">
            <span
              className={`flex size-12 shrink-0 items-center justify-center rounded-full ${o.conflicts > 0 ? "bg-warning-muted text-warning" : "bg-success-muted text-success"}`}
            >
              {o.conflicts > 0 ? <TriangleAlertIcon className="size-6" /> : <CheckIcon className="size-6" />}
            </span>
            <div className="flex min-w-64 flex-1 flex-col gap-0.5">
              <h2 className="text-lg font-semibold">
                {o.conflicts > 0 ? m.overview_conflicts({ n: o.conflicts }) : m.overview_synced()}
              </h2>
              <p className="text-muted-foreground">
                {o.lastVersion && o.lastVersionGame
                  ? m.overview_last({
                      games: o.games,
                      when: relative(o.lastVersion.createdAt),
                      device: o.lastVersionDevice ?? "",
                      game: o.lastVersionGame,
                    })
                  : m.overview_none()}
              </p>
            </div>
          </div>
          <div className="grid grid-cols-[repeat(auto-fit,minmax(180px,1fr))] gap-3">
            <Tile
              label={m.tile_devices()}
              value={m.tile_devices_value({ online: o.devicesOnline, total: o.devices })}
            />
            <Tile label={m.tile_versions()} value={`${o.versions} · ${bytes(o.storedBytes)}`} />
            <Tile
              label={m.tile_mirror()}
              value={
                !o.mirror.enabled
                  ? m.mirror_off()
                  : o.mirror.lastError
                    ? m.mirror_error()
                    : o.mirror.pending > 0
                      ? m.mirror_pending({ n: o.mirror.pending })
                      : m.mirror_current()
              }
            />
            <Tile label={m.tile_rule()} value={m.tile_rule_value()} />
          </div>
        </Card>
      )}

      {conflicts.length > 0 && (
        <section
          role="alert"
          className="flex flex-col gap-3 rounded-lg border border-amber-500/35 bg-warning-muted px-4 py-3.5"
        >
          <div className="flex gap-3">
            <TriangleAlertIcon className="mt-0.5 size-4 shrink-0 text-warning" />
            <div>
              <h2 className="text-sm font-medium text-warning">
                {m.conflicts_title({ n: conflicts.length })}
              </h2>
              <p className="text-sm text-muted-foreground">{m.conflicts_text()}</p>
            </div>
          </div>
          <div className="flex flex-wrap gap-2 pl-7">
            {conflicts.map((g) => (
              <Button key={g.id} size="sm" variant="outline" asChild>
                <Link to="/games/$gameId" params={{ gameId: g.id }}>
                  {g.id}
                </Link>
              </Button>
            ))}
          </div>
        </section>
      )}

      <Card>
        <CardHeader>
          <div className="flex flex-col gap-0.5">
            <CardTitle>{m.recent_title()}</CardTitle>
            <CardDescription>{m.recent_text()}</CardDescription>
          </div>
          <Link to="/games" className="ml-auto text-[13px] text-muted-foreground hover:text-foreground">
            {m.recent_all()} →
          </Link>
        </CardHeader>
        <CardContent className="px-3">
          {games.data ? <GameTable games={games.data.slice(0, 8)} /> : <Skeleton className="h-48" />}
        </CardContent>
      </Card>
    </>
  );
}
