import { labelTone, type LabelColor } from "../../lib/labelColors";

interface Props {
  label: { name: string; color: LabelColor };
  size?: "sm" | "md";
  onRemove?: () => void;
}

export function LabelChip({ label, size = "md", onRemove }: Props) {
  const tone = labelTone(label.color);
  const textSize = size === "sm" ? "text-caption" : "text-meta";
  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-pill px-2.5 py-1 ${textSize} font-medium leading-none`}
      style={{ background: tone.bg, color: tone.fg }}
    >
      {label.name}
      {onRemove && (
        <button
          type="button"
          aria-label={`Remove ${label.name} label`}
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onRemove();
          }}
          className="ml-0.5 leading-none hover:opacity-70"
        >
          ×
        </button>
      )}
    </span>
  );
}
