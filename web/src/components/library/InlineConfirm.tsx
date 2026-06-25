import { useEffect, useRef } from "react";

interface InlineConfirmProps {
  action: string;
  tone: string;
  disabled: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  caption: string;
}

/** A menu's confirm step, which takes the place of its last rows. */
export function InlineConfirm({ action, tone, disabled, onConfirm, onCancel, caption }: InlineConfirmProps) {
  const ref = useRef<HTMLButtonElement>(null);
  // A button that mounts disabled cannot take autoFocus, so focus it once it is enabled.
  useEffect(() => {
    if (!disabled) ref.current?.focus();
  }, [disabled]);
  return (
    <div className="space-y-1">
      <div className="flex items-center gap-2">
        <button
          ref={ref}
          type="button"
          role="menuitem"
          onClick={onConfirm}
          disabled={disabled}
          className={`px-2 py-1 rounded-button border font-semibold hover:opacity-90 disabled:opacity-50 ${tone}`}
        >
          {action}
        </button>
        <button
          type="button"
          role="menuitem"
          onClick={onCancel}
          className="px-2 py-1 rounded-button text-mut hover:text-ink"
        >
          Cancel
        </button>
      </div>
      <p className="text-caption text-mut">{caption}</p>
    </div>
  );
}
