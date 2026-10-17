import { useMutation, useQueryClient } from "@tanstack/react-query";
import { HistoryIcon } from "lucide-react";
import type { Alert } from "@/bindings/Alert";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";
import { dateTime } from "@/lib/format";
import { m } from "@/paraglide/messages";

/** A newer save another program replaced with an older one, and the way back. */
function AlertRow({ alert }: { alert: Alert }) {
  const client = useQueryClient();
  const refresh = () => client.invalidateQueries({ queryKey: ["game", alert.game] });
  const restore = useMutation({
    mutationFn: () => api.restore(alert.game, alert.deviceId, alert.lostVersion),
    onSuccess: refresh,
  });
  const dismiss = useMutation({ mutationFn: () => api.dismissAlert(alert.id), onSuccess: refresh });
  return (
    <section
      role="alert"
      aria-label={m.alert_title()}
      className="flex flex-col gap-3 rounded-lg border border-amber-500/35 bg-warning-muted px-4 py-3.5"
    >
      <div className="flex gap-3">
        <HistoryIcon className="mt-0.5 size-4 shrink-0 text-warning" />
        <div className="flex flex-col gap-0.5">
          <h2 className="text-sm font-medium text-warning">{m.alert_title()}</h2>
          <p className="text-sm text-muted-foreground">
            {m.alert_text({
              device: alert.deviceName,
              when: alert.lostAt ? dateTime(alert.lostAt) : "–",
            })}
          </p>
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-2 pl-7">
        <Button size="sm" disabled={restore.isPending || restore.isSuccess} onClick={() => restore.mutate()}>
          {m.alert_restore({ device: alert.deviceName })}
        </Button>
        <Button size="sm" variant="ghost" disabled={dismiss.isPending} onClick={() => dismiss.mutate()}>
          {m.alert_dismiss()}
        </Button>
        {restore.isSuccess && (
          <span className="text-[13px] text-success">{m.alert_sent({ device: alert.deviceName })}</span>
        )}
        {restore.error && <span className="text-[13px] text-destructive">{restore.error.message}</span>}
      </div>
    </section>
  );
}

export function OverwriteAlerts({ alerts }: { alerts: Alert[] }) {
  return (
    <>
      {alerts.map((a) => (
        <AlertRow key={a.id} alert={a} />
      ))}
    </>
  );
}
