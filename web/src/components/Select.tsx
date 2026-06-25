import { type ReactNode, type SelectHTMLAttributes } from "react";
import { FIELD_SURFACE, fieldClassName } from "./field";

type Size = "md" | "sm" | "toolbar";

interface SelectProps extends Omit<
  SelectHTMLAttributes<HTMLSelectElement>,
  "className" | "size"
> {
  size?: Size;
  children: ReactNode;
  /** Layout only (width, margin, display). */
  className?: string;
}

// The native arrow ignores padding and hugs the border, so draw a chevron inset by the same --select-inset as the text.
const CHEVRON =
  "appearance-none bg-[image:var(--select-chevron)] bg-no-repeat bg-[length:10px_auto] bg-[position:right_var(--select-inset)_center] pr-[calc(var(--select-inset)_+_20px)]";

const SIZE: Record<Size, string> = {
  md: fieldClassName("md", "[--select-inset:calc(var(--spacing)*3)]"),
  sm: fieldClassName("sm", "[--select-inset:calc(var(--spacing)*2)]"),
  toolbar: `${FIELD_SURFACE} [--select-inset:calc(var(--spacing)*2.5)] h-8.5 px-2.5 text-ui font-semibold shadow-card transition-colors hover:border-mut focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue disabled:cursor-wait disabled:opacity-70`,
};

export function Select({
  size = "md",
  className = "",
  children,
  ...rest
}: SelectProps) {
  return (
    <select
      className={`${SIZE[size]} ${CHEVRON} ${className}`.trim()}
      {...rest}
    >
      {children}
    </select>
  );
}
