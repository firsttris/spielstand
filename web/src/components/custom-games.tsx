import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import type { CustomGameRow } from "@/bindings/CustomGameRow";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input, Label, Textarea } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { api } from "@/lib/api";
import { type CustomGameForm, emptyForm, toCustomGame, toForm } from "@/lib/custom-games";
import { parseLines } from "@/lib/lines";
import { m } from "@/paraglide/messages";

/** One custom game: where it is in effect, its paths, and what can be done with it. */
function CustomGameItem({ row, onEdit }: { row: CustomGameRow; onEdit: () => void }) {
  const client = useQueryClient();
  const refresh = () => client.invalidateQueries({ queryKey: ["customGames"] });
  const takeOver = useMutation({
    mutationFn: () => api.takeOverCustomGame(row.game.name),
    onSuccess: refresh,
  });
  const remove = useMutation({ mutationFn: () => api.removeCustomGame(row.game.name), onSuccess: refresh });
  const error = takeOver.error ?? remove.error;
  return (
    <li
      className="flex flex-col gap-2 border-t border-border/50 py-3 first:border-0"
      aria-label={row.game.name}
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-medium">{row.game.name}</span>
        {row.proposedBy ? (
          <Badge variant="warning">{m.custom_proposed({ device: row.proposedByName ?? "?" })}</Badge>
        ) : (
          <Badge variant="success">{m.custom_everywhere()}</Badge>
        )}
        {row.game.extend && <Badge variant="outline">{m.custom_extends()}</Badge>}
        <span className="ml-auto flex flex-wrap gap-1.5">
          {row.proposedBy && (
            <Button size="sm" onClick={() => takeOver.mutate()} disabled={takeOver.isPending}>
              {m.custom_take_over()}
            </Button>
          )}
          <Button size="sm" variant="outline" onClick={onEdit}>
            {m.custom_edit()}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => remove.mutate()} disabled={remove.isPending}>
            {m.custom_remove()}
          </Button>
        </span>
      </div>
      <ul className="flex flex-col gap-0.5 font-mono text-xs text-muted-foreground">
        {row.game.files.map((f) => (
          <li key={f}>{f}</li>
        ))}
      </ul>
      {error && <p className="text-[13px] text-destructive">{error.message}</p>}
    </li>
  );
}

/** Adds a custom game for every device, or changes one (a proposal is taken over with it). */
function CustomGameEditor({ form, setForm }: { form: CustomGameForm; setForm: (f: CustomGameForm) => void }) {
  const client = useQueryClient();
  const save = useMutation({
    mutationFn: () => api.putCustomGame(toCustomGame(form)),
    onSuccess: () => {
      setForm(emptyForm);
      return client.invalidateQueries({ queryKey: ["customGames"] });
    },
  });
  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        save.mutate();
      }}
    >
      <div className="flex flex-wrap gap-3">
        <div className="flex min-w-60 flex-[2_1_240px] flex-col gap-1.5">
          <Label htmlFor="custom-name">{m.custom_name()}</Label>
          <Input
            id="custom-name"
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            required
          />
        </div>
        <div className="flex min-w-48 flex-[1_1_200px] flex-col gap-1.5">
          <Label htmlFor="custom-kind">{m.custom_kind()}</Label>
          <Select
            value={form.extend ? "extend" : "new"}
            onValueChange={(v) => setForm({ ...form, extend: v === "extend" })}
          >
            <SelectTrigger id="custom-kind">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="new">{m.custom_kind_new()}</SelectItem>
              <SelectItem value="extend">{m.custom_kind_extend()}</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="custom-paths">{m.custom_paths()}</Label>
        <Textarea
          id="custom-paths"
          className="font-mono text-[13px]"
          rows={Math.max(2, parseLines(form.paths).length + 1)}
          value={form.paths}
          onChange={(e) => setForm({ ...form, paths: e.target.value })}
          placeholder="<xdgData>/Game/saves"
          required
        />
      </div>
      <div className="flex items-center gap-2">
        <Button type="submit" variant="outline" disabled={save.isPending}>
          {m.custom_save()}
        </Button>
        {save.error && <span className="text-[13px] text-destructive">{save.error.message}</span>}
      </div>
    </form>
  );
}

/** Games Spielstand does not find on its own, and devices' proposals waiting to be taken over. */
export function CustomGames() {
  const games = useQuery({ queryKey: ["customGames"], queryFn: api.customGames });
  const [form, setForm] = useState<CustomGameForm>(emptyForm);
  return (
    <Card>
      <CardHeader>
        <div className="flex flex-col gap-0.5">
          <CardTitle>{m.custom_title()}</CardTitle>
          <CardDescription>{m.custom_text()}</CardDescription>
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {games.data?.length === 0 && <p className="text-sm text-muted-foreground">{m.custom_empty()}</p>}
        {games.data && games.data.length > 0 && (
          <ul aria-label={m.custom_title()}>
            {games.data.map((row) => (
              <CustomGameItem key={row.game.name} row={row} onEdit={() => setForm(toForm(row.game))} />
            ))}
          </ul>
        )}
        <CustomGameEditor form={form} setForm={setForm} />
      </CardContent>
    </Card>
  );
}
