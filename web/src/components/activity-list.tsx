import type { Activity } from "@/bindings/Activity";
import type { EventKind } from "@/bindings/EventKind";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { m } from "@/paraglide/messages";

const eventText: Record<EventKind, () => string> = {
  uploaded: m.event_uploaded,
  branched: m.event_branched,
  keptOwn: m.event_kept_own,
  resolved: m.event_resolved,
  restoreRequested: m.event_restore_requested,
  started: m.event_started,
  stopped: m.event_stopped,
  paired: m.event_paired,
  removed: m.event_removed,
  imported: m.event_imported,
  other: m.event_other,
};

/** Events, newest first; `when` formats the time column (relative or absolute). */
export function ActivityList({
  items,
  when,
  className,
}: {
  items: Activity[];
  when: (iso: string) => string;
  className?: string;
}) {
  return (
    <ol className={cn("flex flex-col", className)}>
      {items.map((a) => (
        <li
          key={a.id}
          className="flex flex-wrap items-center gap-3 border-t border-border/50 py-2.5 text-sm first:border-0"
        >
          <span className="w-32 shrink-0 text-xs text-muted-foreground">{when(a.at)}</span>
          <span className="min-w-48 flex-1">
            {a.game && <span className="font-medium">{a.game}: </span>}
            {eventText[a.kind]()}
          </span>
          {a.deviceName && <Badge className="h-5 px-1.5 text-[11px]">{a.deviceName}</Badge>}
        </li>
      ))}
    </ol>
  );
}
