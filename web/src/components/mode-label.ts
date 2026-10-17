import type { SaveMode } from "@/bindings/SaveMode";
import { m } from "@/paraglide/messages";

export const modeName = (mode: SaveMode) => (mode === "sync" ? m.mode_sync() : m.mode_backup());

export const modeText = (mode: SaveMode) => (mode === "sync" ? m.mode_sync_text() : m.mode_backup_text());
