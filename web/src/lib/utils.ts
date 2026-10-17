import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** Two letters for a game tile, e.g. "Baldur's Gate 3" → "BG". */
export function initials(title: string): string {
  const words = title
    .replace(/[^\p{L}\p{N} ]/gu, " ")
    .split(/\s+/)
    .filter(Boolean);
  const letters =
    words.length > 1 ? `${words[0]?.[0] ?? ""}${words[1]?.[0] ?? ""}` : (words[0]?.slice(0, 2) ?? "?");
  return letters.toUpperCase();
}

/** A stable tile colour per game, from a small palette that reads on dark and light. */
export function tileColor(title: string): string {
  const palette = ["#fca5a5", "#fde68a", "#86efac", "#fdba74", "#93c5fd", "#c4b5fd", "#f9a8d4", "#5eead4"];
  let hash = 0;
  for (const c of title) hash = (hash * 31 + (c.codePointAt(0) ?? 0)) >>> 0;
  return palette[hash % palette.length] ?? "#a1a1aa";
}
