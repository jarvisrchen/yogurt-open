import { describe, expect, it } from "vitest";
import { heldKey, hotkeyKeys, isFnHotkey } from "./hotkey";

describe("hotkeyKeys", () => {
  it("draws each stored shape as key caps", () => {
    expect(hotkeyKeys("fn")).toEqual(["fn"]);
    expect(hotkeyKeys("option+e")).toEqual(["⌥", "E"]);
    expect(hotkeyKeys("right_option")).toEqual(["Right ⌥"]);
    expect(hotkeyKeys("ctrl + space")).toEqual(["⌃", "Space"]);
    expect(hotkeyKeys("command+shift+1")).toEqual(["⌘", "⇧", "1"]);
    expect(hotkeyKeys("right_option+m")).toEqual(["Right ⌥", "M"]);
    expect(hotkeyKeys("Left_Command + Shift + k")).toEqual(["Left ⌘", "⇧", "K"]);
  });

  it("spots Fn in either spelling", () => {
    expect(isFnHotkey("Globe")).toBe(true);
    expect(isFnHotkey("option+e")).toBe(false);
  });

  it("draws a held modifier without its side", () => {
    expect(heldKey("right_option")).toBe("⌥");
    expect(heldKey("left_command")).toBe("⌘");
    expect(heldKey("fn")).toBe("fn");
  });
});
