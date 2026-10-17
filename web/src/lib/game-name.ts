/** A game as the server lists it: its id, and a name when the id is not one (Switch games are
 * keyed by title id, `Switch 0100F2C0115B6000`). */
type Named = { id: string; title: string | null };

/** The name to show for a game. */
export function gameName(game: Named): string {
  return game.title ?? game.id;
}

/** Names by id, for lists that carry only ids (activity, the last version). */
export function namesById(games: readonly Named[] | undefined): (id: string) => string {
  const titles = new Map((games ?? []).map((g) => [g.id, gameName(g)]));
  return (id) => titles.get(id) ?? id;
}
