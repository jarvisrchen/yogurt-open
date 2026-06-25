import { useState } from "react";
import { NavLink, useLocation, useNavigate } from "react-router";
import { ChevronRight, Plus, Star } from "lucide-react";
import { useCreateMeeting } from "../../lib/api/meetings";
import { isArchivedLabel, useCreateLabel, useLabels, useReorderLabels } from "../../lib/api/labels";
import { LabelCap, useLabelCap } from "../labels/LabelCap";
import { SidebarLabelRow } from "./SidebarLabelRow";
import { keepSearch } from "./libraryReturn";
import { Button } from "../Button";
import { LABEL } from "../settings/labelClass";
import { getLabelSortMode, setLabelSortMode, sortLabels } from "../../lib/labelSort";
import { INLINE_INPUT } from "../field";
import { navRow, useNavPill } from "../NavPill";
import { Slot, slots } from "../../slots";

const navClass =
  (layout: string, lit = false) =>
  ({ isActive }: { isActive: boolean }) =>
    `px-3 py-1.5 rounded-button text-ui ${layout} ${navRow(isActive ? "pill" : lit ? "lit" : "idle")}`;

/** The Meetings side of the sidebar, its own component so its pill's hook mounts and unmounts with the rows it measures, as `VoiceNav` does. */
export function LibraryNav() {
  const navigate = useNavigate();
  const createMeeting = useCreateMeeting();
  const labels = useLabels();
  const createLabel = useCreateLabel();
  const labelCap = useLabelCap();
  const reorderLabels = useReorderLabels();
  const [addingLabel, setAddingLabel] = useState(false);
  const [newLabelName, setNewLabelName] = useState("");
  const [sortMode, setSortMode] = useState(getLabelSortMode);
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const [archivedOpen, setArchivedOpen] = useState(false);
  const [openedFor, setOpenedFor] = useState<string | null>(null);
  const allSorted = sortLabels(labels.data ?? [], sortMode);
  const sortedLabels = allSorted.filter((l) => !isArchivedLabel(l));
  const archivedLabels = allSorted.filter(isArchivedLabel);
  const { pathname, search } = useLocation();
  const carry = keepSearch(search);
  const inArchive = pathname.startsWith("/archive");
  const { group, pill } = useNavPill(pathname, {
    inset: "inset-x-2",
    labels: [...sortedLabels, ...(archivedOpen ? archivedLabels : [])].map((l) => l.id).join(),
  });
  // Set during render, so the selected row never paints hidden: the group opens once per arrival on one of its labels, and can be closed there.
  // Cleared on leaving, since the sidebar stays mounted across pages and Back to the same label is a new arrival.
  const onArchivedLabel = archivedLabels.some((l) => pathname === `/archive/label/${l.id}`);
  if (onArchivedLabel && openedFor !== pathname) {
    setOpenedFor(pathname);
    setArchivedOpen(true);
  } else if (!onArchivedLabel && openedFor !== null) setOpenedFor(null);

  function handleDrop(targetIndex: number) {
    if (dragIndex === null || dragIndex === targetIndex) {
      setDragIndex(null);
      return;
    }
    // Reorder numbers every id it gets, so the hidden archived labels keep their slots around the move.
    const ids = allSorted.map((l) => l.id);
    const moved = sortedLabels[dragIndex].id;
    ids.splice(ids.indexOf(moved), 1);
    const target = ids.indexOf(sortedLabels[targetIndex].id);
    ids.splice(dragIndex < targetIndex ? target + 1 : target, 0, moved);
    reorderLabels.mutate(ids);
    setDragIndex(null);
  }

  function commitNewLabel() {
    const name = newLabelName.trim();
    if (name.length > 0) {
      // A name already taken returns that label, so an archived one opens its group to show where it went.
      createLabel.mutate(
        { name },
        {
          onSuccess: (l) => {
            if (archivedLabels.some((a) => a.id === l.id)) setArchivedOpen(true);
          },
        },
      );
    }
    setNewLabelName("");
    setAddingLabel(false);
  }

  // Named only for its own trip, so the button glides into the live page's recording control.
  const [starting, setStarting] = useState(false);

  const handleNew = async () => {
    try {
      const m = await createMeeting.mutateAsync(undefined);
      setStarting(true);
      navigate(`/meeting/${m.id}`, { state: { autoStart: true } });
    } catch (e) {
      console.error("create meeting failed", e);
    }
  };

  return (
    <>
      <div className="px-4 pb-3">
        <Button
          onClick={handleNew}
          disabled={createMeeting.isPending}
          className="w-full"
          style={starting ? { viewTransitionName: "record-control" } : undefined}
        >
          <Plus size={16} aria-hidden />
          New meeting
        </Button>
      </div>

      <div ref={group} className="relative">
        {pill}
        <nav className="px-2 flex flex-col gap-0.5">
          <NavLink to={`/${carry}`} end className={navClass("")}>
            All meetings
          </NavLink>
          <NavLink to={`/starred${carry}`} className={navClass("flex items-center gap-1.5")}>
            <Star size={14} className="shrink-0 fill-current" aria-hidden />
            Starred
          </NavLink>
          <Slot of={slots.librarySidebar.nav} />
        </nav>

        <div className={`relative px-5 pt-5 pb-1 flex items-center justify-between ${LABEL}`}>
          <span>Labels</span>
          <div className="flex items-center gap-1">
            <select
              aria-label="Sort labels"
              value={sortMode}
              onChange={(e) => {
                const mode = e.target.value as typeof sortMode;
                setSortMode(mode);
                setLabelSortMode(mode);
              }}
              className="text-caption text-mut bg-transparent border-none text-right focus:outline-none cursor-pointer"
            >
              <option value="alpha">A-Z</option>
              <option value="updated">Last updated</option>
              <option value="custom">Custom</option>
            </select>
            <button
              type="button"
              aria-label="New label"
              onClick={() => {
                setNewLabelName("");
                setAddingLabel(labelCap === null || !addingLabel);
              }}
              disabled={createLabel.isPending}
              className="text-mut hover:text-ink disabled:opacity-50"
            >
              <Plus size={12} aria-hidden />
            </button>
          </div>
        </div>
        <div className="px-2 flex flex-col gap-0.5">
          {addingLabel && labelCap !== null && <LabelCap count={labelCap} className="px-3 py-1" />}
          {addingLabel && labelCap === null && (
            <div className="px-3 py-1">
              <input
                autoFocus
                placeholder="Label name"
                value={newLabelName}
                onChange={(e) => setNewLabelName(e.target.value)}
                onBlur={commitNewLabel}
                disabled={createLabel.isPending}
                onKeyDown={(e) => {
                  if (e.key === "Enter") commitNewLabel();
                  if (e.key === "Escape") {
                    setNewLabelName("");
                    setAddingLabel(false);
                  }
                }}
                className={INLINE_INPUT}
              />
            </div>
          )}
          {sortedLabels.length === 0 && archivedLabels.length === 0 && !addingLabel ? (
            <p className="px-3 py-1 text-meta text-mut">No labels yet</p>
          ) : (
            sortedLabels.map((l, i) => (
              <div
                key={l.id}
                draggable={sortMode === "custom"}
                onDragStart={() => setDragIndex(i)}
                onDragOver={(e) => {
                  if (sortMode === "custom") e.preventDefault();
                }}
                onDrop={() => handleDrop(i)}
                onDragEnd={() => setDragIndex(null)}
                className={dragIndex === i ? "opacity-50" : undefined}
              >
                <SidebarLabelRow label={l} inArchive={inArchive} />
              </div>
            ))
          )}
          {archivedLabels.length > 0 && (
            <>
              <button
                type="button"
                aria-expanded={archivedOpen}
                onClick={() => setArchivedOpen(!archivedOpen)}
                className="flex items-center gap-1 px-3 py-1.5 rounded-button text-meta text-mut hover:text-ink"
              >
                <ChevronRight
                  size={12}
                  aria-hidden
                  className={`shrink-0 transition-transform ${archivedOpen ? "rotate-90" : ""}`}
                />
                <span className="flex-1 text-left">Archived labels</span>
                {/* The trailing gap lines the count up with the label rows' counts, which sit left of their options button. */}
                <span className="text-caption mr-5.5">{archivedLabels.length}</span>
              </button>
              {archivedOpen && archivedLabels.map((l) => <SidebarLabelRow key={l.id} label={l} inArchive />)}
            </>
          )}
        </div>
        <Slot of={slots.librarySidebar.groups} />
      </div>
    </>
  );
}
