import { getLocale } from "@/paraglide/runtime";

const units: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 3600],
  ["month", 30 * 24 * 3600],
  ["week", 7 * 24 * 3600],
  ["day", 24 * 3600],
  ["hour", 3600],
  ["minute", 60],
];

/** "vor 3 Minuten" / "3 minutes ago"; under a minute: "gerade eben" / "just now". */
export function relative(iso: string, now: Date = new Date()): string {
  const seconds = Math.round((new Date(iso).getTime() - now.getTime()) / 1000);
  const fmt = new Intl.RelativeTimeFormat(getLocale(), { numeric: "auto" });
  for (const [unit, size] of units) {
    if (Math.abs(seconds) >= size) return fmt.format(Math.round(seconds / size), unit);
  }
  return fmt.format(0, "minute");
}

export function dateTime(iso: string): string {
  return new Intl.DateTimeFormat(getLocale(), {
    weekday: "short",
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(iso));
}

export function bytes(n: number): string {
  const fmt = new Intl.NumberFormat(getLocale(), { maximumFractionDigits: 1 });
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${fmt.format(n / 1024)} KB`;
  if (n < 1024 ** 3) return `${fmt.format(n / 1024 ** 2)} MB`;
  return `${fmt.format(n / 1024 ** 3)} GB`;
}

export function duration(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.round((seconds % 3600) / 60);
  return h > 0 ? `${h} h ${String(m).padStart(2, "0")} min` : `${m} min`;
}
