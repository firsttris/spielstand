import { describe, expect, it } from "vitest";
import { diffFiles, shortPath } from "./diff";

const f = (path: string, sha256: string) => ({ path, sha256, size: 1 });

describe("diffFiles", () => {
  it("names what loading a version does", () => {
    const changes = diffFiles(
      [f("a", "1"), f("b", "2"), f("gone", "3")],
      [f("a", "1"), f("b", "9"), f("new", "4")],
    );
    expect(changes).toEqual([
      { path: "b", change: "replaced" },
      { path: "new", change: "added" },
      { path: "gone", change: "removed" },
      { path: "a", change: "same" },
    ]);
  });

  it("shortens paths to the recognisable tail", () => {
    expect(shortPath("<root:steam>/steamapps/compatdata/1/pfx/Saves/slot.sav")).toBe("Saves/slot.sav");
    expect(shortPath("a/b")).toBe("a/b");
  });
});
