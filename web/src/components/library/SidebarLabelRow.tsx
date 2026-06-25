import { useEffect, useRef, useState } from "react";
import { NavLink, useLocation, useNavigate } from "react-router";
import { MoreHorizontal } from "lucide-react";
import {
  useDeleteLabel,
  useUpdateLabel,
  type PaletteColor,
  type LabelWithCount,
} from "../../lib/api/labels";
import { InlineConfirm } from "./InlineConfirm";
import { keepSearch } from "./libraryReturn";
import { DEFAULT_CUSTOM_COLOR, isHexColor, labelTone } from "../../lib/labelColors";
import { INLINE_INPUT } from "../field";
import { navRow } from "../NavPill";
import { Slot, slots } from "../../slots";

const SWATCH_COLORS: PaletteColor[] = ["blue", "matcha", "straw", "lilac", "honey", "slate"];

interface Props {
  label: LabelWithCount;
  /** The row opens and counts the label's archived meetings, in the Archive view and under Archived labels. */
  inArchive: boolean;
}

export function SidebarLabelRow({ label, inArchive }: Props) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  // "slot" is a Pro menu item's own confirm, which replaces Delete while it shows.
  const [confirming, setConfirming] = useState<"delete" | "slot" | null>(null);
  const [draftName, setDraftName] = useState(label.name);
  const [customColor, setCustomColor] = useState(
    isHexColor(label.color) ? label.color : DEFAULT_CUSTOM_COLOR,
  );
  const wrapperRef = useRef<HTMLDivElement>(null);
  const update = useUpdateLabel();
  const del = useDeleteLabel();
  const { pathname, search } = useLocation();
  const navigate = useNavigate();
  const live = label.meeting_count - label.archived_count;

  useEffect(() => {
    if (!menuOpen) return;
    setCustomColor(isHexColor(label.color) ? label.color : DEFAULT_CUSTOM_COLOR);
    const onDown = (e: MouseEvent) => {
      if (wrapperRef.current && !wrapperRef.current.contains(e.target as Node)) {
        setMenuOpen(false);
        setConfirming(null);
      }
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [menuOpen, label.color]);

  function commitCustomColor(hex: string) {
    if (isHexColor(hex)) update.mutate({ id: label.id, color: hex });
  }

  // Only Delete times out: a Pro item's confirm may wait on the server and show an error.
  useEffect(() => {
    if (confirming !== "delete") return;
    const t = setTimeout(() => setConfirming(null), 3000);
    return () => clearTimeout(t);
  }, [confirming]);

  const tone = labelTone(label.color);

  function commitRename() {
    const name = draftName.trim();
    if (name.length > 0 && name !== label.name) {
      update.mutate({ id: label.id, name });
    }
    setRenaming(false);
  }

  if (renaming) {
    return (
      <div className="px-3 py-1">
        <input
          autoFocus
          value={draftName}
          onChange={(e) => setDraftName(e.target.value)}
          onBlur={commitRename}
          onKeyDown={(e) => {
            if (e.key === "Enter") commitRename();
            if (e.key === "Escape") {
              setDraftName(label.name);
              setRenaming(false);
            }
          }}
          className={INLINE_INPUT}
        />
      </div>
    );
  }

  const base = inArchive ? "/archive" : "";
  const path = `${base}/label/${label.id}`;
  const carry = keepSearch(search);

  return (
    <div ref={wrapperRef} className="relative group/label">
      <NavLink
        to={path + carry}
        onClick={(e) => {
          if (menuOpen) e.preventDefault();
          // A modified click keeps its browser meaning, such as a new tab.
          else if (pathname === path && e.button === 0 && !(e.metaKey || e.ctrlKey || e.shiftKey || e.altKey)) {
            e.preventDefault();
            navigate((base || "/") + carry);
          }
        }}
        className={({ isActive }) =>
          `flex items-center gap-2 px-3 py-1.5 rounded-button text-ui ${navRow(isActive ? "pill" : "idle")}`
        }
      >
        <span className="w-2.25 h-2.25 rounded-[3px] shrink-0" style={{ background: tone.fg }} aria-hidden />
        <span className="flex-1 truncate">{label.name}</span>
        <span className="text-caption text-mut">{inArchive ? label.archived_count : live}</span>
        <button
          type="button"
          aria-label={`${label.name} label options`}
          onMouseDown={(e) => e.stopPropagation()}
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            setMenuOpen((o) => !o);
          }}
          className="shrink-0 text-mut hover:text-ink opacity-0 group-hover/label:opacity-100 focus-visible:opacity-100"
        >
          <MoreHorizontal size={14} aria-hidden />
        </button>
      </NavLink>
      {menuOpen && (
        <div
          role="menu"
          className="absolute right-0 top-full mt-1 bg-card border border-line rounded-card shadow-pop py-2 px-3 min-w-50 z-20 text-ui"
        >
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setMenuOpen(false);
              setDraftName(label.name);
              setRenaming(true);
            }}
            className="block w-full text-left py-1 text-ink hover:text-blue"
          >
            Rename
          </button>
          <div className="flex items-center gap-1.5 py-1.5">
            {SWATCH_COLORS.map((c) => {
              const swatch = labelTone(c);
              return (
                <button
                  key={c}
                  type="button"
                  aria-label={`Set color ${c}`}
                  onClick={() => update.mutate({ id: label.id, color: c })}
                  className={`w-4 h-4 rounded-[5px] ${
                    label.color === c ? "ring-2 ring-offset-1 ring-blue" : ""
                  }`}
                  style={{ background: swatch.fg }}
                />
              );
            })}
          </div>
          <div className="flex items-center gap-1.5 pb-1.5">
            <input
              type="color"
              aria-label="Custom label color"
              // Falls back past a half-typed hex: browsers render an invalid color input value as black.
              value={
                isHexColor(label.color)
                  ? label.color
                  : isHexColor(customColor)
                    ? customColor
                    : DEFAULT_CUSTOM_COLOR
              }
              onChange={(e) => {
                setCustomColor(e.target.value);
                update.mutate({ id: label.id, color: e.target.value });
              }}
              className="w-4 h-4 rounded-[5px] border-none bg-transparent p-0 cursor-pointer"
            />
            <input
              type="text"
              aria-label="Custom label color hex"
              value={customColor}
              onChange={(e) => setCustomColor(e.target.value)}
              onBlur={() => commitCustomColor(customColor)}
              onKeyDown={(e) => {
                if (e.key === "Enter") commitCustomColor(customColor);
              }}
              placeholder="#a3c9ff"
              className="w-20 px-1.5 py-0.5 rounded-button border border-line bg-paper text-caption text-ink focus:outline-none focus:ring-2 focus:ring-blue/30"
            />
          </div>
          {confirming === "delete" ? (
            <InlineConfirm
              action="Delete?"
              tone="bg-strsoft text-ink border-straw/40"
              disabled={del.isPending}
              onConfirm={async () => {
                setMenuOpen(false);
                setConfirming(null);
                await del.mutateAsync(label.id);
              }}
              onCancel={() => setConfirming(null)}
              caption={`Removes the label from ${label.meeting_count} meeting${label.meeting_count === 1 ? "" : "s"}`}
            />
          ) : (
            <>
              <Slot
                of={slots.labelMenu}
                label={label}
                confirming={confirming === "slot"}
                onConfirming={(on) => setConfirming(on ? "slot" : null)}
                close={() => {
                  setMenuOpen(false);
                  setConfirming(null);
                }}
              />
              {confirming !== "slot" && (
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => setConfirming("delete")}
                  className="block w-full text-left py-1 text-straw hover:opacity-80"
                >
                  Delete
                </button>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
