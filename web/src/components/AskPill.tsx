import { useKeyboardShortcut } from "../hooks/useKeyboardShortcut";
import { FLOATING } from "./ChatWindow";

interface AskPillProps {
  onExpand: () => void;
  label?: string;
  position?: string;
}

/** Shares ChatWindow's bottom-center anchor and is the 48px strip its grow starts from, so the two read as one surface. */
export function AskPill({ onExpand, label = "Ask this meeting…", position = FLOATING }: AskPillProps) {
  useKeyboardShortcut({ key: "k", metaOrCtrl: true }, onExpand);
  return (
    <button
      type="button"
      onClick={onExpand}
      aria-label={label.replace("…", "")}
      className={`${position} h-12 flex items-center justify-between gap-3 px-4 py-2 bg-[var(--color-card)] border border-[var(--color-line)] rounded-full shadow-pop text-left text-body text-[var(--color-mut)] hover:border-[var(--color-blue)] hover:text-[var(--color-ink)] transition-colors z-30`}
    >
      <span className="flex-1 truncate">{label}</span>
      <span
        aria-hidden="true"
        className="inline-flex [@media(pointer:coarse)]:hidden items-center justify-center h-6 px-2 rounded-chip text-caption font-mono text-[var(--color-mut)] bg-[var(--color-blsoft)]"
      >
        ⌘K
      </span>
      <span
        aria-hidden="true"
        className="inline-flex items-center justify-center h-7 w-7 rounded-full bg-[var(--color-blue)] text-white"
      >
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
          <path
            d="M6 9.5V2.5M6 2.5L2.5 6M6 2.5L9.5 6"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </span>
    </button>
  );
}
