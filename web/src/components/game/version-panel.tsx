import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { DownloadIcon, ShieldCheckIcon } from "lucide-react";
import { useState } from "react";
import type { GameDetail } from "@/bindings/GameDetail";
import type { VersionRow } from "@/bindings/VersionRow";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input, Label } from "@/components/ui/input";
import { Separator } from "@/components/ui/misc";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { api } from "@/lib/api";
import { type Change, diffFiles, shortPath } from "@/lib/diff";
import { dateTime } from "@/lib/format";
import { m } from "@/paraglide/messages";
import { versionText } from "./version-label";

const changeBadge: Record<
  Change,
  { variant: "default" | "info" | "warning" | "success"; text: () => string }
> = {
  same: { variant: "default", text: m.change_same },
  replaced: { variant: "info", text: m.change_replaced },
  added: { variant: "success", text: m.change_added },
  removed: { variant: "warning", text: m.change_removed },
};

export function VersionPanel({
  game,
  row,
  onRestore,
}: {
  game: GameDetail;
  row: VersionRow;
  onRestore: () => void;
}) {
  const client = useQueryClient();
  const devices = useQuery({ queryKey: ["devices"], queryFn: api.devices, refetchInterval: 5_000 });
  const online = (devices.data ?? []).filter((d) => d.online);
  const [target, setTarget] = useState<string>();
  const device = online.find((d) => d.id === target) ?? online[0];
  const onTarget = game.versions.find((v) => device && v.onDevices.includes(device.name));
  const compareTo = onTarget ?? game.versions.find((v) => v.isHead);
  const changes = diffFiles(compareTo?.meta.files ?? [], row.meta.files);
  const [label, setLabel] = useState(row.label ?? "");

  const refresh = () => client.invalidateQueries({ queryKey: ["game", game.id] });
  const restore = useMutation({
    mutationFn: () => api.restore(game.id, device?.id ?? "", row.meta.id),
    onMutate: onRestore,
    onSuccess: refresh,
  });
  const pin = useMutation({
    mutationFn: () => api.patchVersion(row.meta.id, { pinned: !row.pinned, label: null }),
    onSuccess: refresh,
  });
  const rename = useMutation({
    mutationFn: () => api.patchVersion(row.meta.id, { pinned: null, label }),
    onSuccess: refresh,
  });
  const resolve = useMutation({ mutationFn: () => api.resolve(game.id, row.meta.id), onSuccess: refresh });

  return (
    <Card className="flex min-w-0 flex-[1_1_360px] flex-col gap-4 p-5" aria-labelledby="selected-version">
      <div className="flex flex-col gap-0.5">
        <span className="text-xs text-muted-foreground">{m.selected_version()}</span>
        <h2 id="selected-version" className="text-base font-semibold">
          {dateTime(row.meta.createdAt)} · {row.deviceName}
        </h2>
        <span className="text-[13px] text-muted-foreground">{versionText(row)}</span>
      </div>
      <Separator />

      <div className="flex flex-col gap-2">
        <span className="font-medium">
          {device ? m.what_happens_on({ device: device.name }) : m.what_happens()}
        </span>
        <ul className="overflow-hidden rounded-lg border">
          {changes.map((c) => (
            <li
              key={c.path}
              className="flex items-center gap-2.5 border-b border-border/50 px-3 py-2 last:border-0"
              title={c.path}
            >
              <Badge
                variant={changeBadge[c.change].variant}
                className="h-5 w-[72px] justify-center px-0 text-[11px]"
              >
                {changeBadge[c.change].text()}
              </Badge>
              <span className="min-w-0 truncate font-mono text-xs">{shortPath(c.path)}</span>
            </li>
          ))}
        </ul>
      </div>

      <p role="note" className="flex gap-2.5 rounded-lg border p-3 text-[13px] text-muted-foreground">
        <ShieldCheckIcon className="mt-0.5 size-4 shrink-0 text-success" />
        {m.restore_safety()}
      </p>
      {game.mode === "sync" && !row.isHead && (
        <p role="note" className="text-[13px] text-muted-foreground">
          {m.restore_continues()}
        </p>
      )}

      {online.length === 0 ? (
        <p className="text-sm text-muted-foreground">{m.restore_no_device()}</p>
      ) : (
        <div className="flex flex-wrap gap-2">
          <Select value={device?.id} onValueChange={setTarget}>
            <SelectTrigger className="min-w-36" aria-label={m.target_device()}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {online.map((d) => (
                <SelectItem key={d.id} value={d.id}>
                  {m.restore_on({ device: d.name })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button className="flex-[1_1_140px]" disabled={restore.isPending} onClick={() => restore.mutate()}>
            {m.restore_button()}
          </Button>
        </div>
      )}
      {restore.isSuccess && (
        <p className="text-sm text-success">{m.restore_sent({ device: device?.name ?? "" })}</p>
      )}
      {restore.error && <p className="text-sm text-destructive">{restore.error.message}</p>}

      {row.isBranch && game.conflict && (
        <Button variant="secondary" disabled={resolve.isPending} onClick={() => resolve.mutate()}>
          {m.resolve_this()}
        </Button>
      )}

      <form
        className="flex flex-wrap items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          rename.mutate();
        }}
      >
        <div className="flex min-w-48 flex-1 flex-col gap-1.5">
          <Label htmlFor="label">{m.label_name()}</Label>
          <Input
            id="label"
            value={label}
            placeholder={m.label_placeholder()}
            onChange={(e) => setLabel(e.target.value)}
          />
        </div>
        <Button type="submit" variant="outline" disabled={rename.isPending}>
          {m.save()}
        </Button>
      </form>
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" onClick={() => pin.mutate()} disabled={pin.isPending}>
          {row.pinned ? m.unpin() : m.pin()}
        </Button>
        <Button variant="ghost" asChild>
          <a href={api.downloadUrl(row.meta.id)} download>
            <DownloadIcon /> {m.download()}
          </a>
        </Button>
      </div>
    </Card>
  );
}
