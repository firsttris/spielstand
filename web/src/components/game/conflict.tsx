import { useMutation, useQueryClient } from "@tanstack/react-query";
import { TriangleAlertIcon } from "lucide-react";
import type { GameDetail } from "@/bindings/GameDetail";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { api } from "@/lib/api";
import { dateTime } from "@/lib/format";
import { m } from "@/paraglide/messages";
import { triggerText } from "./version-label";

/** Played on two PCs: the head and each open branch side by side, one click to choose. */
export function ConflictChooser({ game }: { game: GameDetail }) {
  const client = useQueryClient();
  const choices = game.versions.filter((v) => v.isHead || v.isBranch);
  const resolve = useMutation({
    mutationFn: (version: string) => api.resolve(game.id, version),
    onSuccess: () => client.invalidateQueries({ queryKey: ["game", game.id] }),
  });

  return (
    <Card
      className="flex flex-col gap-4 border-amber-500/40 p-5"
      role="region"
      aria-labelledby="conflict-title"
    >
      <div className="flex gap-3">
        <TriangleAlertIcon className="mt-0.5 size-5 shrink-0 text-warning" />
        <div className="flex flex-col gap-0.5">
          <h2 id="conflict-title" className="font-semibold">
            {m.conflict_title()}
          </h2>
          <p className="text-sm text-muted-foreground">{m.conflict_text()}</p>
        </div>
      </div>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(240px,1fr))] gap-3">
        {choices.map((v) => (
          <section
            key={v.meta.id}
            aria-label={v.deviceName}
            className="flex flex-col gap-3 rounded-lg border p-4"
          >
            <span className="font-semibold">{v.deviceName}</span>
            <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[13px]">
              <dt className="text-muted-foreground">{m.conflict_when()}</dt>
              <dd>{dateTime(v.meta.createdAt)}</dd>
              <dt className="text-muted-foreground">{m.conflict_why()}</dt>
              <dd>{triggerText(v)}</dd>
              <dt className="text-muted-foreground">{m.conflict_files()}</dt>
              <dd>{v.meta.files.length}</dd>
            </dl>
            <Button size="sm" disabled={resolve.isPending} onClick={() => resolve.mutate(v.meta.id)}>
              {m.conflict_choose()}
            </Button>
          </section>
        ))}
      </div>
    </Card>
  );
}
