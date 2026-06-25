import type { QueryClient } from "@tanstack/react-query";
import { createElement, type ComponentType, type ReactNode, type RefObject, type UIEvent } from "react";
import type { RouteObject } from "react-router";
import type { Label, LabelWithCount } from "./lib/api/labels";
import type { Meeting, MeetingSummary } from "./lib/api/meetings";
import type { AddWord } from "./components/voice/DictationCard";

type List<P = object> = readonly ComponentType<P>[];

export interface SettingsSectionSlot {
  id: string;
  label: string;
  Component: ComponentType;
  /** Beside the label in the Settings nav. */
  Badge?: ComponentType<{ active: boolean }>;
}

/** A listener on the Shell's one app-wide `/ws` socket. */
export interface AppSocketSlot {
  /** Each time the socket opens, since frames sent while it was down are lost. */
  open: (qc: QueryClient) => void;
  frame: (type: string, frame: unknown, qc: QueryClient) => void;
}

/** What the Voice page's stats show, for its header. `devices` is set while they sum every device, which hides this Mac's history. */
export interface VoiceShown {
  count: number;
  devices: number | null;
}

export type EnhanceVersion = "previous" | "new";

/**
 * `recording` offers Update now and Pause, `stopped` only Pause, and `final` (the post-meeting page
 * while the summary generates) neither, showing the recap only while bullets exist.
 */
export type RecapMode = "recording" | "stopped" | "final";

export interface DockRecap {
  /** `null` until the first state lands. */
  state: { bullets: readonly unknown[] } | null;
  /** The mode the view shows in, `undefined` for no Recap view. */
  mode: RecapMode | undefined;
  panel: (p: {
    listRef: RefObject<HTMLDivElement | null>;
    onScroll: (e: UIEvent<HTMLDivElement>) => void;
    onJump: (ts_ms: number) => void;
    fontSize: number;
  }) => ReactNode;
}

export interface Slots {
  routes: RouteObject[];
  librarySidebar: { nav: List; groups: List };
  voiceSidebar: List;
  voicePage: List<{ filter: string | null; onShown: (s: VoiceShown | null) => void }>;
  /** A hook for the dictation cards' Add to dictionary, or `undefined` to leave the button out. */
  useAddWord: () => AddWord | undefined;
  settingsSections: readonly SettingsSectionSlot[];
  /** Keyed by settings section id. */
  settingsRows: Record<"model" | "audio" | "voice" | "general" | "about", List>;
  meetingPostActions: List<{ meetingId: string; meeting?: Meeting }>;
  /** Beside Re-enhance: the note format it asks for, `"auto"` to let the model pick. */
  noteFormat: List<{ value: string; onChange: (value: string) => void; disabled?: boolean }>;
  /** Above the notes while a re-enhance still holds the notes it replaced. */
  enhanceCompare: List<{
    showing: EnhanceVersion;
    onShow: (version: EnhanceVersion) => void;
    onKeep: (version: EnhanceVersion) => void;
    busy: boolean;
  }>;
  /** Above the post page's title row. */
  meetingPostTop: List<{ meetingId: string | null; token: string | null }>;
  meetingLive: List<{ meetingId: string | null; token: string | null; meeting?: Meeting; recording: boolean }>;
  /** The transcript dock's Recap view: a hook, which the dock calls on every render with the mode its page asks for. */
  useDockRecap: (meetingId: string | null, token: string | null, mode: RecapMode | undefined) => DockRecap;
  /** Columns beside the live page's microphone controls. Each is a flex item. */
  meetingAudio: List<{ meetingId: string; recording: boolean }>;
  /** Replaces the live page's model pill while the meeting has no enhance yet. */
  meetingLlm: List<{ llmModel: string; disabled: boolean }>;
  meetingCard: List<{ meeting: MeetingSummary }>;
  libraryActions: {
    header: List<{ archived: boolean; labelId?: string }>;
    /** Menu items in the card's actions menu. `close` shuts the menu. */
    card: List<{ id: string; title: string; labels: Label[]; archived: boolean; isLive: boolean; close: () => void }>;
    selection: List<{
      meetings: Pick<Meeting, "id" | "title" | "labels">[];
      archived: boolean;
      activeId: string | null;
      onClear: () => void;
    }>;
  };
  /**
   * A sidebar label's menu items, above Delete. `confirming` is set while one shows its own confirm in Delete's place.
   * shortcut: every item shares the one `confirming`, so a second item that confirms needs its own key.
   */
  labelMenu: List<{
    label: LabelWithCount;
    confirming: boolean;
    onConfirming: (on: boolean) => void;
    close: () => void;
  }>;
  shellPills: List;
  /** After the Library header's meeting count. */
  libraryGreeting: List;
  /** A hook for what replaces the sidebars' "Local-only · on" badge, or `null` to leave it. */
  useSidebarBadge: () => string | null;
  appSocket: readonly AppSocketSlot[];
}

const empty: Slots = {
  routes: [],
  librarySidebar: { nav: [], groups: [] },
  voiceSidebar: [],
  voicePage: [],
  useAddWord: () => undefined,
  settingsSections: [],
  settingsRows: { model: [], audio: [], voice: [], general: [], about: [] },
  meetingPostActions: [],
  noteFormat: [],
  enhanceCompare: [],
  meetingPostTop: [],
  meetingLive: [],
  useDockRecap: () => ({ state: null, mode: undefined, panel: () => null }),
  meetingAudio: [],
  meetingLlm: [],
  meetingCard: [],
  libraryActions: { header: [], card: [], selection: [] },
  labelMenu: [],
  shellPills: [],
  libraryGreeting: [],
  useSidebarBadge: () => null,
  appSocket: [],
};

// The free source build deletes ./pro, and the glob is then {}.
const pro = import.meta.glob<{ default: Partial<Slots> }>("./pro/index.ts", { eager: true });
const fill = Object.values(pro)[0]?.default ?? {};

export const slots: Slots = {
  ...empty,
  ...fill,
  librarySidebar: { ...empty.librarySidebar, ...fill.librarySidebar },
  libraryActions: { ...empty.libraryActions, ...fill.libraryActions },
  settingsRows: { ...empty.settingsRows, ...fill.settingsRows },
};

export function Slot<P extends object>({ of, ...props }: { of: readonly ComponentType<P>[] } & P): ReactNode {
  return of.map((C, i) => createElement(C, { ...(props as P), key: i }));
}
