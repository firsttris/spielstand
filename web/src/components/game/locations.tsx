import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { CheckIcon, CircleAlertIcon, MonitorIcon } from "lucide-react";
import { useState } from "react";
import type { GameDetail } from "@/bindings/GameDetail";
import type { GameLocation } from "@/bindings/GameLocation";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Dot } from "@/components/ui/misc";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { api } from "@/lib/api";
import { type Need, type Standing, viewLocation } from "@/lib/locations";
import { m } from "@/paraglide/messages";

const standingBadge: Record<
  Standing,
  { variant: "success" | "default" | "outline" | "warning"; text: () => string }
> = {
  current: { variant: "success", text: m.loc_current },
  behind: { variant: "default", text: m.loc_behind },
  diverged: { variant: "warning", text: m.loc_diverged },
  unknown: { variant: "outline", text: m.loc_unknown },
  none: { variant: "outline", text: m.loc_none },
};

const sources: Record<string, () => string> = {
  steam: m.source_steam,
  prefixr: m.source_prefixr,
  gameFolder: m.source_gameFolder,
  heroic: m.source_heroic,
  lutris: m.source_lutris,
  wine: m.source_wine,
  ludusavi: m.source_ludusavi,
  custom: m.source_custom,
};

/** Asks for the game's prefix on one device: one of the candidates, or a folder to type in.
 * For a Switch game the candidates are the save folders of the emulators that hold it. */
function ChoosePrefix({
  game,
  location,
  candidates,
  emulator = false,
}: {
  game: string;
  location: GameLocation;
  candidates: string[];
  emulator?: boolean;
}) {
  const client = useQueryClient();
  const [prefix, setPrefix] = useState(candidates[0] ?? "");
  const save = useMutation({
    mutationFn: () => api.setPrefix(game, location.deviceId, prefix),
    onSuccess: () => client.invalidateQueries({ queryKey: ["game", game] }),
  });
  const id = `prefix-${location.deviceId}`;
  return (
    <form
      className="flex flex-col gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        save.mutate();
      }}
    >
      <label htmlFor={id} className="text-[13px] text-muted-foreground">
        {emulator
          ? m.problem_several_emulators()
          : candidates.length > 0
            ? m.problem_several()
            : m.problem_no_prefix()}
      </label>
      <div className="flex flex-wrap gap-2">
        {candidates.length > 0 ? (
          <Select value={prefix} onValueChange={setPrefix}>
            <SelectTrigger id={id} className="min-w-0 flex-[1_1_280px] font-mono text-[13px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {candidates.map((c) => (
                <SelectItem key={c} value={c} className="font-mono text-[13px]">
                  {c}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : (
          <Input
            id={id}
            className="flex-[1_1_280px] font-mono text-[13px]"
            value={prefix}
            onChange={(e) => setPrefix(e.target.value)}
            placeholder={m.prefix_placeholder()}
            required
          />
        )}
        <Button type="submit" variant="outline" disabled={save.isPending || !prefix.trim()}>
          {emulator ? m.emulator_save() : m.prefix_save()}
        </Button>
      </div>
      {save.error && <p className="text-[13px] text-destructive">{save.error.message}</p>}
    </form>
  );
}

function NeedNote({ game, location, need }: { game: string; location: GameLocation; need: Need }) {
  switch (need.kind) {
    case "nothing":
      return null;
    case "choosePrefix":
      return <ChoosePrefix game={game} location={location} candidates={need.candidates} />;
    case "chooseEmulator":
      return <ChoosePrefix game={game} location={location} candidates={need.candidates} emulator />;
    case "startInEmulator":
      return (
        <p className="flex gap-2 text-[13px] text-warning">
          <CircleAlertIcon className="mt-0.5 size-3.5 shrink-0" />
          {m.problem_start_in_emulator({ emulator: need.emulator })}
        </p>
      );
    case "failed":
      return (
        <p className="flex gap-2 text-[13px] text-destructive">
          <CircleAlertIcon className="mt-0.5 size-3.5 shrink-0" />{" "}
          {m.problem_failed({ message: need.message })}
        </p>
      );
    case "proposed":
      return (
        <p className="flex gap-2 text-[13px] text-warning">
          <CircleAlertIcon className="mt-0.5 size-3.5 shrink-0" />
          <span>
            {m.loc_proposed()}{" "}
            <Link to="/settings" className="underline underline-offset-2">
              {m.loc_proposed_link()}
            </Link>
          </span>
        </p>
      );
    case "foreignSync":
      return (
        <p className="flex gap-2 text-[13px] text-warning">
          <CircleAlertIcon className="mt-0.5 size-3.5 shrink-0" />
          {need.tool === "stellwerk" ? m.foreign_stellwerk() : m.foreign_syncthing({ folder: need.folder })}
        </p>
      );
  }
}

function LocationRow({
  game,
  location,
  branches,
}: {
  game: string;
  location: GameLocation;
  branches: ReadonlySet<string>;
}) {
  const view = viewLocation(location, branches);
  const badge = standingBadge[view.standing];
  const source = location.report?.source;
  return (
    <li
      className="flex flex-col gap-2 border-t border-border/50 py-3 first:border-0"
      aria-label={location.deviceName}
    >
      <div className="flex flex-wrap items-center gap-2">
        <MonitorIcon className="size-4 text-muted-foreground" />
        <span className="font-medium">{location.deviceName}</span>
        <Dot className={`size-1.5 ${location.online ? "bg-emerald-400" : "bg-muted-foreground/40"}`} />
        {!location.online && <span className="text-xs text-muted-foreground">{m.loc_offline()}</span>}
        <span className="ml-auto flex flex-wrap items-center gap-1.5">
          {source && <Badge variant="outline">{(sources[source] ?? (() => source))()}</Badge>}
          {view.backupOnly && <Badge variant="warning">{m.loc_backup_only()}</Badge>}
          <Badge variant={badge.variant}>
            {view.standing === "current" && <CheckIcon />} {badge.text()}
          </Badge>
        </span>
      </div>
      {location.report?.location && (
        <code
          className="truncate pl-6 font-mono text-xs text-muted-foreground"
          title={location.report.location}
        >
          {location.report.location}
        </code>
      )}
      <div className="pl-6 empty:hidden">
        <NeedNote game={game} location={location} need={view.need} />
      </div>
    </li>
  );
}

/** Every device with what it holds of the game, where, and what (if anything) it needs. */
export function Locations({ game }: { game: GameDetail }) {
  if (game.locations.length === 0) return null;
  const branches = new Set(game.versions.filter((v) => v.isBranch).map((v) => v.meta.id));
  return (
    <Card>
      <CardHeader>
        <div className="flex flex-col gap-0.5">
          <CardTitle>{m.locations_title()}</CardTitle>
          <CardDescription>{m.locations_text()}</CardDescription>
        </div>
      </CardHeader>
      <CardContent>
        <ul className="flex flex-col">
          {game.locations.map((l) => (
            <LocationRow key={l.deviceId} game={game.id} location={l} branches={branches} />
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}
