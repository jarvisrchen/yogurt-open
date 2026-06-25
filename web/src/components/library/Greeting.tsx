import { useGreeting } from "../../hooks/useGreeting";
import { formatCount } from "./pagination";
import { Shimmer } from "../Shimmer";
import { Slot, slots } from "../../slots";

interface Props {
  count?: number;
  /** The Archive view's header: its own title, the archived count, and nothing after it. */
  archived?: boolean;
}

export function Greeting({ count, archived = false }: Props) {
  const { greeting } = useGreeting();
  const plural = count === 1 ? "meeting" : "meetings";
  return (
    <header>
      <h1 className="heading-greeting">{archived ? "Archive" : greeting}</h1>
      <p className="mt-1 flex flex-wrap items-center gap-1.5 text-ui text-mut">
        {count === undefined ? (
          <span className="h-[1.5em] inline-flex items-center"><Shimmer className="h-2.5 w-37.5" inline /></span>
        ) : (
          <span>
            {archived
              ? `${formatCount(count)} archived ${plural}`
              : `${formatCount(count)} ${plural} · all on this Mac`}
          </span>
        )}
        {!archived && <Slot of={slots.libraryGreeting} />}
      </p>
    </header>
  );
}
