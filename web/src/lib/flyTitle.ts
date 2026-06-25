/**
 * Flies `name` from `src` into the heading `dest`, which the caller renames meanwhile.
 * The old heading blurs out while the name flies, so the date it keeps never overlaps the incoming name.
 * Resolves once `renamed` settles and the heading shows again, and rejects with its error after putting the old heading back.
 */
export async function flyTitle(src: HTMLElement, dest: HTMLElement, name: string, renamed: Promise<unknown>): Promise<void> {
  const from = src.getBoundingClientRect();
  const to = dest.getBoundingClientRect();
  const look = getComputedStyle(dest);
  // An empty easing throws, and jsdom has no theme to read.
  const EASE_POP = look.getPropertyValue("--ease-pop").trim() || "ease-out";
  const EASE_SLIDE = look.getPropertyValue("--ease-slide").trim() || "ease-out";
  const fly = document.createElement("div");
  fly.textContent = name;
  fly.setAttribute("aria-hidden", "true");
  Object.assign(fly.style, {
    position: "fixed",
    left: `${to.left + parseFloat(look.paddingLeft)}px`,
    top: `${to.top + parseFloat(look.paddingTop)}px`,
    margin: "0",
    zIndex: "50",
    pointerEvents: "none",
    whiteSpace: "nowrap",
    transformOrigin: "0 0",
    font: look.font,
    letterSpacing: look.letterSpacing,
    color: look.color,
  });
  document.body.append(fly);
  const scale = parseFloat(getComputedStyle(src).fontSize) / parseFloat(look.fontSize);
  // `old-out` in index.css, which WAAPI cannot name.
  const out = dest.animate([{}, { opacity: 0, transform: "translateY(-4px)", filter: "blur(2px)" }], {
    duration: 160,
    easing: EASE_POP,
    fill: "forwards",
  });
  const flight = fly.animate(
    [
      { transform: `translate(${from.left - to.left}px, ${from.top - to.top}px) scale(${scale})` },
      { transform: "none" },
    ],
    { duration: 340, easing: EASE_SLIDE, fill: "both" },
  );
  try {
    await Promise.all([flight.finished, renamed]);
  } finally {
    out.cancel();
    // The heading fades back under the landed name, so only its date visibly arrives.
    dest.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 160, easing: EASE_POP });
    void fly
      .animate([{ opacity: 1 }, { opacity: 0 }], { duration: 160, easing: EASE_POP, fill: "forwards" })
      .finished.finally(() => fly.remove());
  }
  const wash = look.getPropertyValue("--color-blsoft");
  dest.animate([{ backgroundColor: wash }, { backgroundColor: wash, offset: 0.3 }, {}], {
    duration: 1100,
    easing: EASE_POP,
  });
}
