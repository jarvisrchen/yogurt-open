interface Props {
  className?: string;
  /** A width computed at render, which Tailwind cannot see as a class. */
  width?: string | number;
  radius?: "rounded" | "rounded-pill" | "rounded-card" | "rounded-[6px]";
  inline?: boolean;
  /** Announces the load. Without it the bar is hidden from screen readers, and the region it sits in says what is loading. */
  label?: string;
}

export function Shimmer({ className = "", width, radius = "rounded", inline = false, label }: Props) {
  return (
    <span
      className={`${inline ? "inline-block align-middle" : "block"} ${radius} shimmer ${className}`}
      style={width === undefined ? undefined : { width }}
      {...(label ? { role: "status", "aria-label": label } : { "aria-hidden": true })}
    />
  );
}
