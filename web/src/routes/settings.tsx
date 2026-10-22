import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import type { SaveMode } from "@/bindings/SaveMode";
import { CustomGames } from "@/components/custom-games";
import { modeText } from "@/components/mode-label";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input, Label } from "@/components/ui/input";
import { NarrowPage, PageHeader } from "@/components/ui/misc";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { api } from "@/lib/api";
import { relative } from "@/lib/format";
import { m } from "@/paraglide/messages";
import { getLocale, type Locale, locales, setLocale } from "@/paraglide/runtime";

export const Route = createFileRoute("/settings")({ component: Settings });

const languageNames: Record<string, string> = { de: "Deutsch", en: "English" };

function Settings() {
  const client = useQueryClient();
  const overview = useQuery({ queryKey: ["overview"], queryFn: api.overview });
  const [source, setSource] = useState("");
  const imported = useMutation({
    mutationFn: () => api.importLudusavi(source),
    onSuccess: () => client.invalidateQueries({ queryKey: ["games"] }),
  });
  const logout = useMutation({
    mutationFn: api.logout,
    onSuccess: () => client.invalidateQueries({ queryKey: ["session"] }),
  });
  const mirror = overview.data?.mirror;
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const defaultMode = useMutation({
    mutationFn: (mode: SaveMode) => api.setDefaultMode(mode),
    onSuccess: (saved) => client.setQueryData(["settings"], saved),
  });

  return (
    <NarrowPage>
      <PageHeader title={m.nav_settings()} />

      <Card className="flex flex-wrap items-center gap-3 p-5">
        <div className="flex min-w-72 flex-1 flex-col gap-0.5">
          <CardTitle>{m.default_mode_title()}</CardTitle>
          <CardDescription>
            {m.default_mode_text()} {settings.data && modeText(settings.data.defaultMode)}
          </CardDescription>
        </div>
        {settings.data && (
          <Select
            value={settings.data.defaultMode}
            onValueChange={(v) => defaultMode.mutate(v as SaveMode)}
            disabled={defaultMode.isPending}
          >
            <SelectTrigger className="w-48" aria-label={m.default_mode_title()}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="sync">{m.mode_sync()}</SelectItem>
              <SelectItem value="backup">{m.mode_backup()}</SelectItem>
            </SelectContent>
          </Select>
        )}
      </Card>

      <CustomGames />

      <Card>
        <CardHeader>
          <div className="flex flex-col gap-0.5">
            <CardTitle>{m.import_title()}</CardTitle>
            <CardDescription>{m.import_text()}</CardDescription>
          </div>
        </CardHeader>
        <CardContent>
          <form
            className="flex flex-wrap items-end gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              imported.mutate();
            }}
          >
            <div className="flex min-w-72 flex-1 flex-col gap-1.5">
              <Label htmlFor="source">{m.import_source()}</Label>
              <Input
                id="source"
                value={source}
                onChange={(e) => setSource(e.target.value)}
                placeholder="gdrive:ludusavi-backup"
                required
              />
            </div>
            <Button type="submit" disabled={imported.isPending}>
              {imported.isPending ? m.import_running() : m.import_start()}
            </Button>
          </form>
          {imported.data && (
            <p className="mt-3 text-sm text-success">
              {m.import_done({
                games: imported.data.games,
                versions: imported.data.versions,
                skipped: imported.data.skipped,
              })}
            </p>
          )}
          {imported.data?.errors.map((e) => (
            <p key={e} className="mt-1 text-xs text-destructive">
              {e}
            </p>
          ))}
          {imported.error && <p className="mt-3 text-sm text-destructive">{imported.error.message}</p>}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex flex-col gap-0.5">
            <CardTitle>{m.mirror_title()}</CardTitle>
            <CardDescription>{m.mirror_text()}</CardDescription>
          </div>
        </CardHeader>
        <CardContent className="text-sm">
          {!mirror?.enabled ? (
            <p className="text-muted-foreground">{m.mirror_how()}</p>
          ) : (
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5">
              <dt className="text-muted-foreground">{m.mirror_target()}</dt>
              <dd className="font-mono text-[13px]">{mirror.remote}</dd>
              <dt className="text-muted-foreground">{m.mirror_last()}</dt>
              <dd>{mirror.lastOk ? relative(mirror.lastOk) : "–"}</dd>
              <dt className="text-muted-foreground">{m.mirror_waiting()}</dt>
              <dd>{mirror.pending}</dd>
              {mirror.lastError && (
                <>
                  <dt className="text-muted-foreground">{m.mirror_error()}</dt>
                  <dd className="text-destructive">{mirror.lastError}</dd>
                </>
              )}
            </dl>
          )}
        </CardContent>
      </Card>

      <Card className="flex flex-wrap items-center gap-3 p-5">
        <div className="flex min-w-60 flex-1 flex-col gap-0.5">
          <CardTitle>{m.language()}</CardTitle>
          <CardDescription>
            {m.version_text({ version: overview.data?.version ?? "" })} ·{" "}
            <a className="underline underline-offset-2" href="https://github.com/firsttris/spielstand">
              Spielstand by Tristan Teufel
            </a>
          </CardDescription>
        </div>
        <Select value={getLocale()} onValueChange={(v) => setLocale(v as Locale)}>
          <SelectTrigger className="w-40" aria-label={m.language()}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {locales.map((l) => (
              <SelectItem key={l} value={l}>
                {languageNames[l] ?? l}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button variant="outline" onClick={() => logout.mutate()}>
          {m.logout()}
        </Button>
      </Card>
    </NarrowPage>
  );
}
