import type { VersionRow } from "@/bindings/VersionRow";
import { duration } from "@/lib/format";
import { m } from "@/paraglide/messages";

export function triggerText(row: VersionRow): string {
  const t = row.meta.trigger;
  const s = row.meta.sessionSeconds;
  switch (t) {
    case "session":
      return s ? m.trigger_session_for({ duration: duration(s) }) : m.trigger_session();
    case "interim":
      return m.trigger_interim();
    case "beforeRestore":
      return m.trigger_before_restore();
    case "catchUp":
      return m.trigger_catch_up();
    case "manual":
      return m.trigger_manual();
    case "import":
      return m.trigger_import();
  }
}

/** What the timeline shows under the date: a given name, or why the version exists. */
export function versionText(row: VersionRow): string {
  const files = m.files_count({ n: row.meta.files.length });
  return row.label ? `${row.label} · ${triggerText(row)}` : `${triggerText(row)} · ${files}`;
}
