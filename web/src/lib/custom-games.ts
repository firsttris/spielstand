import type { CustomGame } from "@/bindings/CustomGame";
import { parseLines } from "@/lib/lines";

/** What the custom-game form holds: paths one per line. Install folders, which only a device
 * proposes, are kept as they are. */
export type CustomGameForm = { name: string; paths: string; extend: boolean; installDir: string[] };

export const emptyForm: CustomGameForm = { name: "", paths: "", extend: false, installDir: [] };

export function toCustomGame(form: CustomGameForm): CustomGame {
  return {
    name: form.name.trim(),
    files: parseLines(form.paths),
    installDir: form.installDir,
    extend: form.extend,
  };
}

export function toForm(game: CustomGame): CustomGameForm {
  return { name: game.name, paths: game.files.join("\n"), extend: game.extend, installDir: game.installDir };
}
