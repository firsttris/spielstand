import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { GameDetail } from "@/bindings/GameDetail";
import type { SaveMode } from "@/bindings/SaveMode";
import { modeName, modeText } from "@/components/mode-label";
import { Card } from "@/components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { api } from "@/lib/api";
import { m } from "@/paraglide/messages";

const DEFAULT = "default";

/** Who moves this game's saves: the default, or a choice for this game. */
export function ModeSelect({ game }: { game: GameDetail }) {
  const client = useQueryClient();
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const change = useMutation({
    mutationFn: (value: string) => api.setGameMode(game.id, value === DEFAULT ? null : (value as SaveMode)),
    onSuccess: () => client.invalidateQueries({ queryKey: ["game", game.id] }),
  });
  const defaultMode = settings.data?.defaultMode ?? game.mode;
  return (
    <Card className="flex flex-wrap items-center gap-3 p-5">
      <div className="flex min-w-64 flex-1 flex-col gap-0.5">
        <h2 className="text-base font-semibold">{m.mode_title()}</h2>
        <p className="text-[13px] text-muted-foreground">{modeText(game.mode)}</p>
      </div>
      <Select
        value={game.modeIsDefault ? DEFAULT : game.mode}
        onValueChange={(value) => change.mutate(value)}
        disabled={change.isPending}
      >
        <SelectTrigger className="min-w-52" aria-label={m.mode_label()}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={DEFAULT}>{m.mode_default({ mode: modeName(defaultMode) })}</SelectItem>
          <SelectItem value="sync">{m.mode_sync()}</SelectItem>
          <SelectItem value="backup">{m.mode_backup()}</SelectItem>
        </SelectContent>
      </Select>
    </Card>
  );
}
