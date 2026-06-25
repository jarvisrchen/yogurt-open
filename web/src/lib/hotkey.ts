// Draws the hotkeys `crates/yogurt-voice/src/hotkey.rs` parses: `fn`, a lone modifier such as
// `right_option`, or a combo such as `option+e` or `right_option+m`.

const SYMBOL: Record<string, string> = {
  control: "⌃",
  ctrl: "⌃",
  option: "⌥",
  alt: "⌥",
  opt: "⌥",
  shift: "⇧",
  command: "⌘",
  cmd: "⌘",
};

const NAMED_KEY: Record<string, string> = {
  space: "Space",
  return: "Return",
  enter: "Return",
  tab: "Tab",
};

/** The key caps to draw for a stored hotkey, such as `["⌥", "E"]` or `["Right ⌥"]`. */
export function hotkeyKeys(hotkey: string): string[] {
  return hotkey
    .trim()
    .toLowerCase()
    .split("+")
    .map((raw) => {
      const part = raw.trim();
      if (part === "fn" || part === "globe") return "fn";
      const side = /^(left|right)_(.+)$/.exec(part);
      if (side && SYMBOL[side[2]]) {
        return `${side[1] === "left" ? "Left" : "Right"} ${SYMBOL[side[2]]}`;
      }
      return SYMBOL[part] ?? NAMED_KEY[part] ?? part.toUpperCase();
    });
}

export function isFnHotkey(hotkey: string): boolean {
  return ["fn", "globe"].includes(hotkey.trim().toLowerCase());
}

/** A modifier held while recording, such as `right_option`, drawn without its side so Left and Right Option both show as ⌥. */
export function heldKey(name: string): string {
  return hotkeyKeys(name)[0].replace(/^(Left|Right) /, "");
}
