import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import { GameTable } from "@/components/game-table";
import { Card, CardContent } from "@/components/ui/card";
import { Input, Label } from "@/components/ui/input";
import { PageHeader, Skeleton } from "@/components/ui/misc";
import { api } from "@/lib/api";
import { m } from "@/paraglide/messages";

export const Route = createFileRoute("/games/")({ component: Games });

function Games() {
  const games = useQuery({ queryKey: ["games"], queryFn: api.games, refetchInterval: 5_000 });
  const [query, setQuery] = useState("");
  const shown = (games.data ?? [])
    .filter((g) => g.id.toLowerCase().includes(query.trim().toLowerCase()))
    .sort((a, b) => Number(b.conflict) - Number(a.conflict) || a.id.localeCompare(b.id));

  return (
    <>
      <PageHeader title={m.nav_games()} description={m.games_subtitle()}>
        <Label htmlFor="search" className="sr-only">
          {m.search_label()}
        </Label>
        <Input
          id="search"
          type="search"
          className="w-60"
          placeholder={m.search_placeholder()}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </PageHeader>
      <Card>
        <CardContent className="px-3 pt-3">
          {games.data ? <GameTable games={shown} /> : <Skeleton className="h-64" />}
        </CardContent>
      </Card>
    </>
  );
}
