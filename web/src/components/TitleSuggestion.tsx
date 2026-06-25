import { ChevronDown, Sparkles, X } from "lucide-react";
import { useEffect, useRef, useState, type RefObject } from "react";
import { useDismissTitleSuggestions, useUpdateMeetingTitle } from "../lib/api/meetings";
import { flyTitle } from "../lib/flyTitle";
import { Fold, reduced } from "./Fold";
import { withName } from "./library/titles";

interface Props {
  meetingId: string;
  title: string;
  startedAt?: number;
  /** Best first. Empty folds the chip away. */
  names: string[];
  /** The heading a picked name flies into. */
  titleRef: RefObject<HTMLElement | null>;
}

const BTN =
  "inline-flex h-6.5 items-center rounded-pill text-mut transition-colors hover:text-ink hover:bg-blue/10 focus-visible:text-ink focus-visible:bg-blue/10 focus-visible:outline-none";
const SEP = <span aria-hidden className="h-3 w-px bg-blue/25" />;

/** The meeting page's offer of enhance's names for a meeting still on its default title. */
export function TitleSuggestion({ meetingId, title, startedAt, names, titleRef }: Props) {
  // Kept while the chip folds shut, after the names have left the cache.
  const [shown, setShown] = useState(names);
  // The chip shrinks away first, then its row folds shut.
  // Both stay set through a slow request, so the chip never unfolds again before the cache catches up.
  const [leaving, setLeaving] = useState(false);
  const [closing, setClosing] = useState(false);
  if (names.length > 0 && names.join("\n") !== shown.join("\n")) {
    setShown(names);
    setLeaving(false);
    setClosing(false);
  }
  // Names already waiting on load just show; only ones that arrive later rise in.
  const [arriving] = useState(names.length === 0);
  const [menuOpen, setMenuOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const rename = useUpdateMeetingTitle();
  const dismissal = useDismissTitleSuggestions();
  const nameRef = useRef<HTMLSpanElement>(null);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    menuRef.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    const onDown = (e: MouseEvent) => {
      if (!wrapperRef.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [menuOpen]);

  const leave = () => {
    setLeaving(true);
    setTimeout(() => setClosing(true), reduced() ? 0 : 100);
  };
  const fail = (e: unknown) => {
    setLeaving(false);
    setClosing(false);
    setError(e instanceof Error ? e.message : String(e));
  };

  const use = (name: string, src: HTMLElement | null) => {
    if (leaving) return;
    setMenuOpen(false);
    setError(null);
    const renamed = rename.mutateAsync({ id: meetingId, title: withName(title, startedAt, name) });
    leave();
    const dest = titleRef.current;
    if (reduced() || !src || !dest) {
      renamed.catch(fail);
      return;
    }
    src.style.visibility = "hidden";
    flyTitle(src, dest, name, renamed).catch((e) => {
      src.style.visibility = "";
      fail(e);
    });
  };

  const dismiss = () => {
    if (leaving) return;
    setMenuOpen(false);
    setError(null);
    leave();
    dismissal.mutate(meetingId, { onError: fail });
  };

  function onMenuKey(e: React.KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      setMenuOpen(false);
      wrapperRef.current?.querySelector<HTMLElement>("[aria-haspopup]")?.focus();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = [...(menuRef.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
    const i = items.indexOf(document.activeElement as HTMLElement);
    items[(i + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length]?.focus();
  }

  const open = names.length > 0 && !closing;
  return (
    <div ref={wrapperRef} className="relative" onKeyDown={menuOpen ? onMenuKey : undefined}>
      <Fold open={open}>
        <div className="pt-1.5">
          {shown.length > 0 && (
            <span
              className={`relative inline-flex max-w-full items-center h-6.5 rounded-pill bg-blsoft text-meta leading-none ${
                leaving
                  ? "motion-safe:animate-chip-out"
                  : arriving
                    ? "motion-safe:animate-[fade-up_260ms_var(--ease-pop)_120ms_both]"
                    : ""
              }`}
            >
              <span className="inline-flex min-w-0 items-center gap-1.5 pl-2.5 pr-1 font-medium text-mut whitespace-nowrap">
                <Sparkles size={12} aria-hidden className="shrink-0 text-blue" />
                Suggested:
                <span ref={nameRef} className="truncate font-semibold text-ink">
                  {shown[0]}
                </span>
              </span>
              <button
                type="button"
                onClick={() => use(shown[0], nameRef.current)}
                className={`${BTN} px-2.5 font-bold text-blue! hover:text-blue`}
              >
                Use
              </button>
              {SEP}
              {shown.length > 1 && (
                <>
                  <button
                    type="button"
                    aria-haspopup="menu"
                    aria-expanded={menuOpen}
                    aria-label="Other titles"
                    onClick={() => setMenuOpen((o) => !o)}
                    className={`${BTN} px-2`}
                  >
                    <ChevronDown
                      size={12}
                      aria-hidden
                      className={`motion-safe:transition-transform motion-safe:duration-160 ${menuOpen ? "rotate-180" : ""}`}
                    />
                  </button>
                  {SEP}
                </>
              )}
              <button
                type="button"
                aria-label="Dismiss suggestion"
                onClick={dismiss}
                className={`${BTN} w-6.5 justify-center mr-0.5`}
              >
                <X size={12} aria-hidden />
              </button>
            </span>
          )}
          {error && <p className="mt-1 text-meta text-straw">{error}</p>}
        </div>
      </Fold>
      {menuOpen && (
        <div
          ref={menuRef}
          role="menu"
          aria-label="Suggested titles"
          className="absolute left-0 top-full mt-1.5 z-20 min-w-70 max-w-[min(360px,calc(100vw-32px))] rounded-card border border-line bg-card py-1 text-ui shadow-pop"
        >
          <p aria-hidden className="px-3 pt-1.5 pb-1 text-caption font-semibold text-mut">
            Suggested titles
          </p>
          {shown.map((name, i) => (
            <button
              key={name}
              type="button"
              role="menuitem"
              onClick={(e) => use(name, e.currentTarget.querySelector("span"))}
              className="w-full flex items-center gap-2.5 px-3 py-1.5 text-left font-semibold text-ink hover:bg-paper focus-visible:bg-paper focus-visible:outline-none"
            >
              <Sparkles size={14} aria-hidden className={`shrink-0 ${i === 0 ? "text-blue" : "text-mut"}`} />
              <span className="min-w-0 truncate">{name}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
