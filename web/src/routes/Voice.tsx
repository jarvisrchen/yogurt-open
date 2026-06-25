import { useEffect, useMemo, useRef, useState } from "react";
import { skipToken, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useLocation, useSearchParams } from "react-router";
import { Settings as SettingsIcon } from "lucide-react";
import { SearchPill } from "../components/library/SearchPill";
import { groupByDay } from "../components/library/dayGroups";
import { formatCount } from "../components/library/pagination";
import { buttonClassName, Button } from "../components/Button";
import { Pill } from "../components/Pill";
import { LABEL } from "../components/settings/labelClass";
import { DictationCard } from "../components/voice/DictationCard";
import { useRiseOrder } from "../components/voice/motion";
import { MicDevicePicker } from "../components/MicDevicePicker";
import { TryVoice } from "../components/voice/TryVoice";
import { ErrorNote, plural, SETTINGS_LINK } from "../components/voice/Panels";
import { useSettings, VOICE_RETENTIONS } from "../lib/api/settings";
import {
  armVoiceTry,
  cleanVoiceTry,
  useDictations,
  voiceTryKey,
  type StyleKind,
  type VoiceTried,
  type VoiceTry,
} from "../lib/api/voice";
import { useEntitlements } from "../lib/entitlements";
import { hotkeyKeys } from "../lib/hotkey";
import { Slot, slots, type VoiceShown } from "../slots";

export function Voice() {
  const { pathname } = useLocation();
  const [params, setParams] = useSearchParams();
  const q = params.get("q") ?? "";
  const app = params.get("app");
  const handsFree = pathname === "/voice/hands-free";
  const filter = handsFree ? "Hands-free" : app;
  const [shown, setShown] = useState<VoiceShown | null>(null);
  const list = useDictations({ q, app, handsFree });
  const addWord = slots.useAddWord();
  const general = useSettings().data?.general;
  const kept = VOICE_RETENTIONS.find((r) => r.value === general?.voice_retention);

  const onQueryChange = (next: string) => {
    setParams(
      (prev) => {
        const p = new URLSearchParams(prev);
        if (next.trim()) p.set("q", next);
        else p.delete("q");
        return p;
      },
      { replace: true },
    );
  };

  const first = list.data?.pages[0];
  const count = !filter && shown ? shown.count : first?.total;
  const devices = filter ? null : (shown?.devices ?? null);
  // Offset pages shift when a dictation lands between two page fetches, so a row can come back twice.
  const dictations = [...new Map(list.data?.pages.flatMap((p) => p.dictations).map((d) => [d.id, d])).values()];
  const groups = groupByDay(dictations, (d) => d.created_at);
  const rise = useRiseOrder(dictations.map((d) => d.id));

  return (
    <main className="@container flex-1 min-w-0 px-12 pt-10 pb-12 max-w-230">
      <div className="flex items-start justify-between gap-4 flex-wrap mb-6">
        <header>
          <h1 className="heading-greeting">{filter ?? "Voice"}</h1>
          <p className="mt-1 flex flex-wrap items-center gap-1.5 text-ui text-mut">
            {count !== undefined &&
              `${formatCount(count)} ${count === 1 ? "dictation" : "dictations"}${!shown && first?.window_hours != null ? ` in the last ${first.window_hours} hours` : ""}`}
            {devices != null ? (
              <Pill tone="blue">On {plural(devices, "device")}</Pill>
            ) : (
              <Pill tone="matcha">On this Mac</Pill>
            )}
          </p>
        </header>
        <div className="flex items-center gap-2 flex-wrap">
          <SearchPill value={q} onChange={onQueryChange} placeholder="Search dictations" width="w-60" />
          <Link to="/settings/voice" className={buttonClassName("secondary", "", "sm")}>
            <SettingsIcon size={14} aria-hidden />
            Voice settings
          </Link>
        </div>
      </div>

      {!filter && (
        <div className="mb-6">
          <MacTryVoice />
        </div>
      )}
      <Slot of={slots.voicePage} filter={filter} onShown={setShown} />

      {devices != null ? (
        <p className="mt-10 text-meta text-mut">Each device keeps its own dictations, so the history is under This Mac.</p>
      ) : (
        <div aria-busy={list.isPlaceholderData} className={[!filter && "mt-10", list.isPlaceholderData && "opacity-60 transition-opacity"].filter(Boolean).join(" ")}>
          {list.error && <ErrorNote what="dictations" error={list.error} />}
          {list.data && dictations.length === 0 && kept?.value !== "never" && (
            <p className="text-ui text-mut">
              {q.trim()
                ? "No matches"
                : first?.history.count
                  ? "No dictations here yet."
                  : "No dictations yet. Hold the shortcut in any app, talk, and let go."}
            </p>
          )}
          <div className="flex flex-col gap-8">
            {groups.map((g) => (
              <section key={g.key}>
                <h2 className={`${LABEL} mb-2`}>{g.label}</h2>
                <ul className="flex flex-col gap-2">
                  {g.meetings.map((d) => (
                    <DictationCard
                      key={d.id}
                      d={d}
                      addWord={addWord}
                      rise={rise.get(d.id)}
                    />
                  ))}
                </ul>
              </section>
            ))}
          </div>
          {list.hasNextPage && (
            <Button
              variant="secondary"
              size="sm"
              className="mt-4"
              disabled={list.isFetchingNextPage}
              onClick={() => list.fetchNextPage()}
            >
              Show more
            </Button>
          )}
        </div>
      )}

      {kept && (
        <p className="mt-8 text-meta text-mut">
          {kept.value === "never"
            ? "Text is not saved, so dictations count only in the stats, and audio is never saved."
            : `Text is kept on this Mac for ${kept.label}, and audio is never saved.`}{" "}
          Change it in{" "}
          <Link to="/settings/voice" className={SETTINGS_LINK}>
            Settings › Voice
          </Link>
          .
        </p>
      )}
    </main>
  );
}

/** Renewed well inside the server's 5 s lease while the field keeps focus. */
const ARM_EVERY_MS = 2000;

function MacTryVoice() {
  const general = useSettings().data?.general;
  const { pro } = useEntitlements();
  const qc = useQueryClient();
  const heard = useQuery<VoiceTry>({ queryKey: voiceTryKey, queryFn: skipToken }).data ?? null;
  // Other, since the Yogurt page is the app in front and is not a known app.
  const [kind, setKind] = useState<StyleKind>("other");
  const [recleaned, setRecleaned] = useState<{ of: VoiceTry; tried: VoiceTried } | null>(null);
  const reclean = useMutation({ mutationFn: (v: { of: VoiceTry; kind: StyleKind }) => cleanVoiceTry(v.of.raw, v.kind) });
  const recleaning = reclean.variables?.of === heard;
  const result = useMemo(
    () => (heard && recleaned?.of === heard ? { ...heard, ...recleaned.tried } : heard),
    [heard, recleaned],
  );

  const renew = useRef<number | undefined>(undefined);
  // One request at a time, so a renewal still in flight can never land after the disarm and hold a real dictation.
  const sent = useRef(Promise.resolve());
  const arm = (k: StyleKind | null) => {
    sent.current = sent.current.then(() => armVoiceTry(k)).catch(() => {});
  };
  // Clicking Clean up as blurs the field first, so `kind` cannot change while it is armed.
  const onFocusChange = (focused: boolean) => {
    window.clearInterval(renew.current);
    arm(focused ? kind : null);
    renew.current = focused ? window.setInterval(() => arm(kind), ARM_EVERY_MS) : undefined;
  };
  useEffect(
    () => () => {
      // A page left with the field focused never fires blur, so leaving it disarms too.
      if (renew.current !== undefined) {
        window.clearInterval(renew.current);
        arm(null);
      }
      // A try is never kept, and the card's kind starts over on Other.
      qc.removeQueries({ queryKey: voiceTryKey });
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  const onKind = (next: StyleKind) => {
    if (next === kind) return;
    setKind(next);
    if (heard) reclean.mutate({ of: heard, kind: next }, { onSuccess: (tried) => setRecleaned({ of: heard, tried }) });
  };

  const keys = hotkeyKeys(general?.voice_hotkey ?? "fn").join("+");
  return (
    <TryVoice
      result={result}
      kind={kind}
      onKind={pro ? onKind : undefined}
      cleaning={recleaning && reclean.isPending}
      error={recleaning && reclean.error ? `Couldn't clean it up again: ${reclean.error.message}` : undefined}
      placeholder={
        general && !general.voice_enabled
          ? "Voice is off. Turn it on in Settings › Voice to try it here."
          : `Click here, hold ${keys}, and talk.`
      }
      mic={<MicDevicePicker voice />}
      onFocusChange={onFocusChange}
    />
  );
}
