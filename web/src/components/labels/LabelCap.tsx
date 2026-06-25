import { useLabels } from "../../lib/api/labels";
import { useEntitlements } from "../../lib/entitlements";

/** Mirrors `labels_max` in the server's limits, which is what refuses the create. */
export const FREE_LABELS = 5;

/** How many labels exist once a build without Pro can create no more, else `null`. */
export function useLabelCap(): number | null {
  const { pro, loading } = useEntitlements();
  const count = useLabels().data?.length ?? 0;
  return !pro && !loading && count >= FREE_LABELS ? count : null;
}

/** A button rather than an anchor because the card picker sits inside a `<Link>`, and anchors cannot nest. */
export function LabelCap({ count, className = "" }: { count: number; className?: string }) {
  return (
    <p data-testid="label-cap" className={`text-caption text-mut ${className}`}>
      {count > FREE_LABELS ? `${count} labels, over the free ${FREE_LABELS}.` : `${count} of ${FREE_LABELS} labels.`}{" "}
      <button
        type="button"
        role="link"
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          window.open("https://yogurt-ai.dev", "_blank", "noreferrer");
        }}
        className="text-blue hover:underline focus-visible:outline-2 focus-visible:outline-blue"
      >
        Get Pro for more
      </button>
    </p>
  );
}
