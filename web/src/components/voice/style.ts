import type { StyleKind, VoiceStyleChoice } from "../../lib/api/voiceTypes";

export const STYLES: { value: VoiceStyleChoice; label: string }[] = [
  { value: "casual", label: "Casual" },
  { value: "normal", label: "Normal" },
  { value: "formal", label: "Formal" },
];

// "yeah that's fine um I can't make the sixth though I'm gonna be away", as the design doc's table cleans it in each style.
export const SAMPLES: Record<VoiceStyleChoice, string> = {
  casual: "yeah that's fine, I can't make the 6th though, I'm gonna be away",
  normal: "Yeah, that's fine. I can't make the 6th though. I'm gonna be away.",
  formal: "Yes, that is fine. I cannot make the 6th, though. I am going to be away.",
};

/** Everything else lists only the apps moved there, since every app yogurt does not know falls to it. */
export function appsLine(kind: StyleKind, apps: { name: string; kind: StyleKind; moved: boolean }[]): string {
  const names = apps.filter((a) => a.kind === kind && (kind !== "other" || a.moved)).map((a) => a.name);
  if (kind !== "other") return names.length ? names.join(", ") : "No apps yet";
  return names.length ? `${names.join(", ")}, and any app not listed above` : "Any app not listed above";
}
