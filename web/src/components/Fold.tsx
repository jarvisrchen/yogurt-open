import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";

interface FoldProps {
  /** False folds the children shut, then unmounts them. */
  open: boolean;
  /** Fold open on mount too, for a row just added. */
  appear?: boolean;
  /** Runs once the fold has shut, which is when a list drops the row from its data. */
  onClosed?: () => void;
  className?: string;
  children: ReactNode;
}

// The length of `.fold`'s grid-template-rows transition in index.css.
// A timer rather than `transitionend`, which never fires under a hidden ancestor or a cancelled transition.
const FOLD_MS = 220;

// Guarded because jsdom has no `matchMedia`, same as `lib/theme.ts`.
export const reduced = () => typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

export function Fold({ open, appear = false, onClosed, className = "", children }: FoldProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [present, setPresent] = useState(open);
  const [phase, setPhase] = useState<"shut" | "opening" | "open">(open && !appear ? "open" : "shut");
  if (open && !present) setPresent(true);

  useLayoutEffect(() => {
    if (!open || !present || phase !== "shut") return;
    // Lay out the folded row first, or the browser sees only the open one and nothing transitions.
    void ref.current?.offsetHeight;
    setPhase("opening");
  }, [open, present, phase]);

  useEffect(() => {
    if (phase !== "opening") return;
    const t = setTimeout(() => setPhase("open"), reduced() ? 0 : FOLD_MS);
    return () => clearTimeout(t);
  }, [phase]);

  // Only `open` turning false starts a fold, so `present` and `onClosed` are read as they stand then.
  useEffect(() => {
    if (open || !present) return;
    setPhase("shut");
    const t = setTimeout(() => {
      setPresent(false);
      onClosed?.();
    }, reduced() ? 0 : FOLD_MS);
    return () => clearTimeout(t);
  }, [open]);

  if (!present) return null;
  return (
    <div
      ref={ref}
      className={`fold ${className}`}
      data-folded={phase === "shut" ? "" : undefined}
      data-folding={phase === "opening" ? "" : undefined}
    >
      <div>{children}</div>
    </div>
  );
}
