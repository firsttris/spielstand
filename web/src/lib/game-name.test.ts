import { describe, expect, it } from "vitest";
import { gameName, namesById } from "./game-name";

describe("game names", () => {
  it("shows a Switch game's title, other games by their id", () => {
    expect(gameName({ id: "Switch 0100F2C0115B6000", title: "Zelda: Tears of the Kingdom" })).toBe(
      "Zelda: Tears of the Kingdom",
    );
    expect(gameName({ id: "Hades", title: null })).toBe("Hades");
  });

  it("maps ids to names, and unknown ids to themselves", () => {
    const name = namesById([{ id: "Switch 0100F2C0115B6000", title: "Zelda" }]);
    expect(name("Switch 0100F2C0115B6000")).toBe("Zelda");
    expect(name("Hades")).toBe("Hades");
    expect(namesById(undefined)("Hades")).toBe("Hades");
  });
});
