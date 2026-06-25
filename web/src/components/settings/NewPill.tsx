import { Pill } from "../Pill";

/** Marks a Settings nav row the user has not set up yet. */
export function NewPill({ active }: { active: boolean }) {
  return (
    <Pill tone="blue" className={`ml-2 font-bold! ${active ? "bg-card!" : ""}`}>
      new
    </Pill>
  );
}
