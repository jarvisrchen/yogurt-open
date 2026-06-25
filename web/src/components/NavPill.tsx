import { useSlidingThumb } from "../lib/useSlidingThumb";

interface Options {
  /** Changes when rows move without the active one changing, like a reorder, so the pill follows without sliding. */
  labels?: string;
  /** Matches the rows' horizontal inset inside `group`. */
  inset: string;
}

/**
 * A row's colors in a sidebar with a nav pill, positioned so it paints above the pill.
 * "pill" is the current row, which the pill paints, and "lit" is one highlighted without the pill, like Archive inside a narrowed archive.
 */
export const navRow = (state: "pill" | "lit" | "idle") =>
  `relative ${state === "idle" ? "text-ink hover:bg-line/40" : `${state === "lit" ? "bg-blsoft " : ""}text-blue font-semibold`}`;

/** A sidebar's active-row highlight, sliding to the row in `group` marked `aria-current="page"`, under rows styled with `navRow`. */
export function useNavPill(value: string, { labels = "", inset }: Options) {
  const { group, thumb } = useSlidingThumb('[aria-current="page"]', value, labels, { axis: "y" });
  const pill = (
    <span
      ref={thumb}
      aria-hidden
      hidden
      className={`nav-pill absolute top-0 ${inset} rounded-button bg-blsoft motion-safe:transition-[transform,height] motion-safe:duration-300 motion-safe:ease-slide`}
    />
  );
  return { group, pill };
}
