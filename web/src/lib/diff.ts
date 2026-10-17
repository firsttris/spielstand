import type { FileEntry } from "@/bindings/FileEntry";

export type Change = "same" | "replaced" | "added" | "removed";

export type FileChange = { path: string; change: Change };

/** What loading `next` does to a disk that holds `current`. */
export function diffFiles(current: FileEntry[], next: FileEntry[]): FileChange[] {
  const now = new Map(current.map((f) => [f.path, f.sha256]));
  const then = new Map(next.map((f) => [f.path, f.sha256]));
  const changes: FileChange[] = [];
  for (const [path, sha] of then) {
    const had = now.get(path);
    changes.push({ path, change: had === undefined ? "added" : had === sha ? "same" : "replaced" });
  }
  for (const path of now.keys()) {
    if (!then.has(path)) changes.push({ path, change: "removed" });
  }
  const order: Record<Change, number> = { replaced: 0, added: 1, removed: 2, same: 3 };
  return changes.sort((a, b) => order[a.change] - order[b.change] || a.path.localeCompare(b.path));
}

/** `<root:steam>/steamapps/…/Game/Save/x.sav` → `Save/x.sav`: the part a person recognises. */
export function shortPath(path: string): string {
  const parts = path.split("/");
  return parts.length <= 2 ? path : parts.slice(-2).join("/");
}
