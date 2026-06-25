import { useSlidingThumb } from "../lib/useSlidingThumb";

const SIZES = {
  md: "px-4 py-1.5 text-ui",
  sm: "px-3 py-1.25 text-meta",
};

interface SegmentedProps<T extends string> {
  label: string;
  options: { value: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
  size?: keyof typeof SIZES;
  /** Stretch across the parent, with equal-width options. */
  fill?: boolean;
  /** Set on a switch that several screens each render, like the Cloud Library's Meetings | Voice, which remounts on a pick, so its thumb still slides. */
  remounts?: boolean;
}

export function Segmented<T extends string>({ label, options, value, onChange, size = "md", fill = false, remounts = false }: SegmentedProps<T>) {
  const labels = options.map((o) => o.label).join("\n");
  const { group, thumb } = useSlidingThumb('[aria-checked="true"]', value, labels, { remember: remounts ? label : undefined });

  return (
    <div
      ref={group}
      role="radiogroup"
      aria-label={label}
      className={`relative ${fill ? "flex" : "inline-flex w-fit"} gap-0.5 rounded-card bg-line/60 p-1`}
    >
      <span
        ref={thumb}
        aria-hidden
        className="absolute inset-y-1 left-0 rounded-button bg-raised shadow-card motion-safe:transition-[transform,width] motion-safe:duration-260 motion-safe:ease-slide"
      />
      {options.map((opt) => {
        const selected = value === opt.value;
        return (
          <button
            key={opt.value}
            type="button"
            role="radio"
            aria-checked={selected}
            onClick={() => onChange(opt.value)}
            className={`relative cursor-pointer rounded-button ${SIZES[size]} ${fill ? "flex-1" : ""} font-semibold transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue ${
              selected ? "text-ink" : "text-mut hover:text-ink"
            }`}
          >
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}
