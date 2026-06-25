import type { VoiceSetupPlan } from "../../lib/api/voiceTypes";

const NAMED = 5;

export function count(n: number, noun: string): string {
  return `${n.toLocaleString("en-US")} ${noun}${n === 1 ? "" : "s"}`;
}

/** "3 words and 1 snippet", leaving out a zero, or "" when both are. */
function both(words: number, snippets: number): string {
  return [words > 0 && count(words, "word"), snippets > 0 && count(snippets, "snippet")].filter(Boolean).join(" and ");
}

/** `Nov 8, 2:10 PM`, in the user's local time. */
export function when(ms: number): string {
  const d = new Date(ms);
  return `${d.toLocaleDateString("en-US", { month: "short", day: "numeric" })}, ${d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })}`;
}

/** The preview's lines, with `here` naming this device: "this Mac" or "this phone". */
export function planLines(plan: VoiceSetupPlan, here: string): string[] {
  const adds = both(plan.add_words, plan.add_snippets);
  const replaced = [
    ...plan.replace_words.map((w) => `Spells "${w.word}" the Cloud's way${w.sounds_like ? `, sounds like "${w.sounds_like}"` : ", with no sounds like"}`),
    ...plan.replace_snippets.map((cue) => `Takes the Cloud's version of the snippet "${cue}"`),
  ];
  const rest = replaced.length - NAMED;
  const keeps = both(plan.keep_words, plan.keep_snippets);
  return [
    adds && `Adds ${adds}`,
    ...replaced.slice(0, rest > 1 ? NAMED : NAMED + 1),
    rest > 1 && `Takes the Cloud's version of ${rest.toLocaleString("en-US")} more`,
    keeps && `Keeps ${keeps} only on ${here}`,
  ].filter((l): l is string => Boolean(l));
}

export function copiedLine(copied: VoiceSetupPlan): string {
  const added = both(copied.add_words, copied.add_snippets);
  const took = copied.replace_words.length + copied.replace_snippets.length;
  return [
    "Copied from the Cloud.",
    added && `Added ${added}.`,
    took > 0 && `Took the Cloud's version of ${took === 1 ? "1 entry" : `${took.toLocaleString("en-US")} entries`}.`,
  ]
    .filter(Boolean)
    .join(" ");
}

export function changes(plan: VoiceSetupPlan): boolean {
  return plan.add_words + plan.add_snippets + plan.replace_words.length + plan.replace_snippets.length > 0;
}
