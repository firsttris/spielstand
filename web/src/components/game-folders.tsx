import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import type { Device } from "@/bindings/Device";
import { Button } from "@/components/ui/button";
import { Label, Textarea } from "@/components/ui/input";
import { api } from "@/lib/api";
import { m } from "@/paraglide/messages";

/** One path per line; blank lines and surrounding spaces do not count. */
export function parseFolders(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

/** The folders on a device whose sub-folders hold games with their own Wine prefix. */
export function GameFolders({ device }: { device: Device }) {
  const client = useQueryClient();
  const [text, setText] = useState(device.gameFolders.join("\n"));
  const save = useMutation({
    mutationFn: () => api.setGameFolders(device.id, parseFolders(text)),
    onSuccess: () => client.invalidateQueries({ queryKey: ["devices"] }),
  });
  const id = `folders-${device.id}`;
  return (
    <form
      className="flex flex-col gap-1.5"
      onSubmit={(e) => {
        e.preventDefault();
        save.mutate();
      }}
    >
      <Label htmlFor={id}>{m.game_folders_title()}</Label>
      <p className="text-xs text-muted-foreground">{m.game_folders_text()}</p>
      <Textarea
        id={id}
        className="font-mono text-[13px]"
        rows={Math.max(2, parseFolders(text).length + 1)}
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder="~/Games"
      />
      <div className="flex items-center gap-2">
        <Button type="submit" size="sm" variant="outline" disabled={save.isPending}>
          {m.game_folders_save()}
        </Button>
        {save.isSuccess && <span className="text-xs text-success">{m.game_folders_saved()}</span>}
        {save.error && <span className="text-xs text-destructive">{save.error.message}</span>}
      </div>
    </form>
  );
}
