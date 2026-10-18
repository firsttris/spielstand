import { describe, expect, it } from "vitest";
import { parseLines } from "./lines";

describe("parseLines", () => {
  it("takes one trimmed entry per line and skips blank ones", () => {
    expect(parseLines(" ~/Games \n\n/run/media/deck/SD/Games\n  ")).toEqual([
      "~/Games",
      "/run/media/deck/SD/Games",
    ]);
    expect(parseLines("")).toEqual([]);
  });
});
