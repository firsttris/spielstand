import { describe, expect, it } from "vitest";
import { toCustomGame, toForm } from "./custom-games";

describe("custom game form", () => {
  it("turns one path per line into a custom game and back, keeping install folders", () => {
    const game = toCustomGame({
      name: " Indie ",
      paths: "<xdgData>/Indie\n\n <winAppData>/Indie ",
      extend: false,
      installDir: ["Indie"],
    });
    expect(game).toEqual({
      name: "Indie",
      files: ["<xdgData>/Indie", "<winAppData>/Indie"],
      installDir: ["Indie"],
      extend: false,
    });
    expect(toForm(game)).toEqual({
      name: "Indie",
      paths: "<xdgData>/Indie\n<winAppData>/Indie",
      extend: false,
      installDir: ["Indie"],
    });
  });
});
