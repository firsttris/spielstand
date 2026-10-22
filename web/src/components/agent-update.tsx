import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import type { Device } from "@/bindings/Device";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";
import { m } from "@/paraglide/messages";

/** Tells that a device's agent is older than the server and updates it, or shows how to. */
export function AgentUpdate({ device }: { device: Device }) {
  const overview = useQuery({ queryKey: ["overview"], queryFn: api.overview, refetchInterval: 10_000 });
  const update = useMutation({ mutationFn: () => api.updateDevice(device.id) });
  const [copied, setCopied] = useState(false);
  if (!device.outdated) return null;
  const command = overview.data?.updateCommand ?? "…";
  const status = () => {
    if (!device.online) return m.update_offline();
    if (device.playing) return m.update_playing();
    if (update.isError) return update.error.message;
    return update.isSuccess ? m.update_sent() : m.update_hint();
  };

  return (
    <div className="flex flex-col gap-2 rounded-md border p-3 text-[13px]">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="warning">{m.update_available()}</Badge>
        <span className="text-muted-foreground">
          {m.update_versions({ from: device.agentVersion, to: overview.data?.version ?? "…" })}
        </span>
      </div>
      {device.canUpdate ? (
        <div className="flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            onClick={() => update.mutate()}
            disabled={!device.online || !!device.playing || update.isPending}
          >
            {m.update_button()}
          </Button>
          <span className="text-muted-foreground">{status()}</span>
        </div>
      ) : (
        <div className="flex flex-col gap-1.5">
          <span className="text-muted-foreground">{m.update_manual()}</span>
          <div className="flex flex-wrap items-center gap-2">
            <code className="min-w-0 flex-1 overflow-x-auto rounded-md border bg-background px-2 py-1.5 font-mono whitespace-nowrap">
              {command}
            </code>
            <Button
              variant="outline"
              size="sm"
              onClick={async () => {
                await navigator.clipboard?.writeText(command);
                setCopied(true);
                setTimeout(() => setCopied(false), 1500);
              }}
            >
              {copied ? m.copied() : m.copy()}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
