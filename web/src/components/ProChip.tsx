import type { ReactNode } from "react";

/** The lemon "Get Pro" chip a build without Pro shows beside a Pro feature. A span, so its caller picks the link or button. */
export function ProChip({ children = "Get Pro", className = "" }: { children?: ReactNode; className?: string }) {
  return (
    <span
      className={`inline-flex items-center rounded-pill bg-lemon px-1.75 py-px text-caption leading-[1.6] font-bold whitespace-nowrap text-lemonink ${className}`}
    >
      {children}
    </span>
  );
}
