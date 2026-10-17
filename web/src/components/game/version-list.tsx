import type { VersionRow } from "@/bindings/VersionRow";
import { Badge } from "@/components/ui/badge";
import { Card, CardDescription, CardTitle } from "@/components/ui/card";
import { bytes, dateTime } from "@/lib/format";
import { cn } from "@/lib/utils";
import { m } from "@/paraglide/messages";
import { versionText } from "./version-label";

export function VersionList({
  versions,
  selected,
  onSelect,
}: {
  versions: VersionRow[];
  selected: string | undefined;
  onSelect: (id: string) => void;
}) {
  return (
    <Card className="flex min-w-0 flex-[1_1_400px] flex-col gap-3 p-5">
      <div className="flex flex-col gap-0.5">
        <CardTitle>{m.versions_title()}</CardTitle>
        <CardDescription>{m.versions_text()}</CardDescription>
      </div>
      <ol className="flex flex-col gap-1">
        {versions.map((row) => {
          const on = row.meta.id === selected;
          return (
            <li key={row.meta.id}>
              <button
                type="button"
                aria-pressed={on}
                onClick={() => onSelect(row.meta.id)}
                className={cn(
                  "flex w-full items-center gap-3 rounded-lg border px-3 py-2.5 text-left text-sm",
                  on ? "border-input bg-accent" : "border-transparent hover:bg-accent/50",
                )}
              >
                <span
                  aria-hidden="true"
                  className={cn(
                    "size-2 shrink-0 rounded-full",
                    row.isHead
                      ? "bg-emerald-400"
                      : row.isBranch
                        ? "bg-amber-400"
                        : on
                          ? "bg-foreground"
                          : "bg-zinc-500",
                  )}
                />
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="flex flex-wrap items-center gap-2">
                    <span className="font-medium">{dateTime(row.meta.createdAt)}</span>
                    <Badge className="h-5 px-1.5 text-[11px]">{row.deviceName}</Badge>
                    {row.isHead && (
                      <Badge variant="success" className="h-5 px-1.5 text-[11px]">
                        {m.badge_head()}
                      </Badge>
                    )}
                    {row.isBranch && (
                      <Badge variant="warning" className="h-5 px-1.5 text-[11px]">
                        {m.badge_branch()}
                      </Badge>
                    )}
                    {row.pinned && (
                      <Badge variant="outline" className="h-5 px-1.5 text-[11px]">
                        {m.badge_pinned()}
                      </Badge>
                    )}
                    {row.onDevices.length > 0 && (
                      <span className="text-[11px] text-muted-foreground">
                        {m.on_devices({ devices: row.onDevices.join(", ") })}
                      </span>
                    )}
                  </span>
                  <span className="truncate text-xs text-muted-foreground">{versionText(row)}</span>
                </span>
                <span className="font-mono text-xs text-muted-foreground">{bytes(row.meta.blobSize)}</span>
              </button>
            </li>
          );
        })}
      </ol>
    </Card>
  );
}
