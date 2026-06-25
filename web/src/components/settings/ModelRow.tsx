import type { ReactNode } from "react";
import { Pill } from "../Pill";

/** `dim` greys out a row that can never be picked. */
export function ModelRow({
  group,
  name,
  recommended = false,
  label,
  checked,
  disabled,
  dim = false,
  onPick,
  action,
  children,
}: {
  group: string;
  name: string;
  recommended?: boolean;
  label?: string;
  checked: boolean;
  disabled: boolean;
  dim?: boolean;
  onPick: () => void;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className={`flex items-center gap-3 px-3 py-2.5 ${dim ? "opacity-60" : ""}`}>
      <label className={`flex flex-1 min-w-0 items-center gap-3 ${disabled ? "" : "cursor-pointer"}`}>
        <input
          type="radio"
          name={group}
          checked={checked}
          disabled={disabled}
          onChange={onPick}
          className="h-4 w-4 shrink-0 accent-blue"
        />
        <span className="flex-1 min-w-0">
          <span className="flex flex-wrap items-baseline gap-x-2">
            <span className="text-ui font-medium text-ink">{name}</span>
            {recommended && (
              <Pill tone="matcha" className="self-center px-2 py-0.5">
                Recommended
              </Pill>
            )}
            {label && <span className="text-meta text-mut">{label}</span>}
          </span>
          <span className="block text-meta text-mut">{children}</span>
        </span>
      </label>
      {action}
    </div>
  );
}
