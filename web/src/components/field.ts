type FieldSize = "md" | "sm";

export const FIELD_SURFACE =
  "rounded-button border border-line bg-paper text-ink";

export const INLINE_INPUT = `${FIELD_SURFACE} w-full px-2 py-1 text-ui placeholder:text-mut focus:outline-none focus:ring-2 focus:ring-blue/30`;

const BASE = `${FIELD_SURFACE} placeholder:text-grey focus:border-blue focus:outline-none`;

const SIZE: Record<FieldSize, string> = {
  md: "px-3 py-2 text-ui",
  sm: "px-2 py-1.5 text-ui",
};

export function fieldClassName(size: FieldSize = "md", extra = ""): string {
  return `${BASE} ${SIZE[size]} ${extra}`.trim();
}
