import { describe, expect, it } from "vitest";
import { parseFolders } from "./game-folders";

describe("parseFolders", () => {
  it("takes one trimmed path per line and skips blank ones", () => {
    expect(parseFolders(" ~/Games \n\n/run/media/deck/SD/Games\n  ")).toEqual([
      "~/Games",
      "/run/media/deck/SD/Games",
    ]);
    expect(parseFolders("")).toEqual([]);
  });
});
