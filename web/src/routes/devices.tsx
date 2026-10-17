import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { MonitorIcon } from "lucide-react";
import { useState } from "react";
import type { Device } from "@/bindings/Device";
import { ActivityList } from "@/components/activity-list";
import { PairDialog } from "@/components/pair-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardTitle } from "@/components/ui/card";
import { Dot, PageHeader, Separator, Skeleton } from "@/components/ui/misc";
import { api } from "@/lib/api";
import { relative } from "@/lib/format";
import { m } from "@/paraglide/messages";

export const Route = createFileRoute("/devices")({ component: Devices });

const LAUNCH_OPTION = "spielstand-agent launch %command%";

function DeviceCard({ device }: { device: Device }) {
  const client = useQueryClient();
  const remove = useMutation({
    mutationFn: () => api.removeDevice(device.id),
    onSuccess: () => client.invalidateQueries({ queryKey: ["devices"] }),
  });
  return (
    <Card
      className={`flex flex-col gap-3.5 p-5 ${device.playing ? "border-blue-500/40" : ""}`}
      aria-labelledby={`dev-${device.id}`}
    >
      <div className="flex items-start gap-3">
        <span className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-secondary">
          <MonitorIcon className="size-5" />
        </span>
        <div className="flex flex-1 flex-col gap-0.5">
          <h2 id={`dev-${device.id}`} className="font-semibold">
            {device.name}
          </h2>
          <span className="text-[13px] text-muted-foreground">
            {m.device_meta({ version: device.agentVersion, os: device.os || device.hostname })}
          </span>
        </div>
        {device.playing ? (
          <Badge variant="info">
            <Dot className="size-1.5 bg-blue-400" /> {m.device_playing()}
          </Badge>
        ) : device.online ? (
          <Badge variant="success">
            <Dot className="size-1.5 bg-emerald-400" /> {m.device_online()}
          </Badge>
        ) : (
          <Badge variant="outline">{m.device_offline()}</Badge>
        )}
      </div>
      <Separator />
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-[13px]">
        <dt className="text-muted-foreground">{m.device_now()}</dt>
        <dd className={device.playing ? "font-medium" : ""}>
          {device.playing
            ? m.device_playing_since({
                game: device.playing,
                since: relative(device.playingSince ?? new Date().toISOString()),
              })
            : m.device_idle()}
        </dd>
        <dt className="text-muted-foreground">{m.device_seen()}</dt>
        <dd>{device.online ? m.device_connected() : device.lastSeen ? relative(device.lastSeen) : "–"}</dd>
        <dt className="text-muted-foreground">{m.device_conflicts()}</dt>
        <dd>{device.pendingConflicts}</dd>
      </dl>
      <Button
        variant="ghost"
        size="sm"
        className="self-start text-muted-foreground"
        onClick={() => window.confirm(m.device_remove_confirm({ device: device.name })) && remove.mutate()}
      >
        {m.device_remove()}
      </Button>
    </Card>
  );
}

function Devices() {
  const devices = useQuery({ queryKey: ["devices"], queryFn: api.devices, refetchInterval: 5_000 });
  const activity = useQuery({
    queryKey: ["activity", 8],
    queryFn: () => api.activity(8),
    refetchInterval: 5_000,
  });
  const [copied, setCopied] = useState(false);

  return (
    <>
      <PageHeader title={m.nav_devices()} description={m.devices_subtitle()}>
        <PairDialog>
          <Button>{m.devices_add()}</Button>
        </PairDialog>
      </PageHeader>

      {!devices.data ? (
        <Skeleton className="h-48" />
      ) : devices.data.length === 0 ? (
        <Card className="p-6 text-center text-muted-foreground">{m.devices_empty()}</Card>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fit,minmax(300px,1fr))] gap-4">
          {devices.data.map((d) => (
            <DeviceCard key={d.id} device={d} />
          ))}
        </div>
      )}

      <Card className="flex flex-wrap items-center gap-3 p-5">
        <div className="flex min-w-72 flex-1 flex-col gap-0.5">
          <CardTitle>{m.launch_title()}</CardTitle>
          <CardDescription>{m.launch_text()}</CardDescription>
        </div>
        <code className="rounded-md border bg-background px-3 py-2 font-mono text-[13px]">
          {LAUNCH_OPTION}
        </code>
        <Button
          variant="outline"
          size="sm"
          onClick={async () => {
            await navigator.clipboard?.writeText(LAUNCH_OPTION);
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          }}
        >
          {copied ? m.copied() : m.copy()}
        </Button>
      </Card>

      <Card>
        <CardContent className="flex flex-col gap-2">
          <CardTitle>{m.activity_title()}</CardTitle>
          <ActivityList items={activity.data ?? []} when={relative} />
        </CardContent>
      </Card>
    </>
  );
}
