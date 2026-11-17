import type { ReactNode } from "react";
import clsx from "clsx";

interface ToastProps {
  icon: ReactNode;
  tone?: keyof typeof TONE;
  title: string;
  body: string;
  actions?: ReactNode;
  onClose?: () => void;
  children?: ReactNode;
  testId?: string;
}

const TONE = {
  blue: "bg-blsoft text-blue",
  matcha: "bg-mtsoft text-matcha",
  straw: "bg-strsoft text-straw font-bold",
  honey: "bg-honeysoft text-honeyink",
  lemon: "bg-lemon text-lemonink",
};

/** The bottom-right notice. A caller inside a transformed parent portals it, since `fixed` would follow that parent. */
export function Toast({ icon, tone = "blue", title, body, actions, onClose, children, testId }: ToastProps) {
  return (
    <div
      role="status"
      data-testid={testId}
      className="fixed bottom-6 right-[calc(var(--dock-clear,0px)+24px)] z-40 w-91 max-w-[calc(100vw-32px)] rounded-card border border-line bg-card pt-3.5 pr-3.5 pb-3 pl-4 shadow-pop motion-safe:animate-pop-up"
    >
      <div className="flex items-start gap-2.5">
        <span aria-hidden className={clsx("grid h-7 w-7 shrink-0 place-items-center rounded-pill", TONE[tone])}>
          {icon}
        </span>
        <div className="min-w-0">
          <p className="text-body font-bold text-ink">{title}</p>
          <p className="mt-0.5 text-meta leading-normal text-mut">{body}</p>
        </div>
        {onClose && (
          <button type="button" onClick={onClose} aria-label="Close"
            className="-mt-0.5 ml-auto px-0.5 text-lead leading-none text-grey hover:text-ink"
          >
            ×
          </button>
        )}
      </div>
      {children}
      {actions && <div className="mt-3 flex items-center gap-1.5 pl-9.5">{actions}</div>}
    </div>
  );
}
