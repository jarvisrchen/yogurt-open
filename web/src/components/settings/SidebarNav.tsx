import { Fragment } from "react";
import { Link } from "react-router";
import { ArrowLeft } from "lucide-react";
import type { DataPaths, ProviderView } from "../../lib/api/settings";
import { Pill } from "../Pill";
import { NewPill } from "./NewPill";
import { useLibraryBack } from "../library/libraryReturn";
import { Shimmer } from "../Shimmer";
import { navRow, useNavPill } from "../NavPill";
import { slots, type SettingsSectionSlot } from "../../slots";

// The 212px width matches the Library sidebar, but the components are deliberately
// not shared because the section lists differ.
// The `→` in the caption is U+2192, never ASCII `->`.

interface Props {
  active: string;
  onChange: (s: string) => void;
  /** The three below are undefined while settings load, and their parts wait without moving the list. */
  providers?: ProviderView[];
  paths?: DataPaths;
  voiceEnabled?: boolean;
}

export const SECTIONS: { id: string; label: string; Badge?: SettingsSectionSlot["Badge"] }[] = [
  { id: "transcription", label: "Transcription" },
  { id: "model", label: "Model" },
  { id: "audio", label: "Audio" },
  { id: "voice", label: "Voice" },
  { id: "general", label: "General" },
  ...slots.settingsSections,
  { id: "import", label: "Import" },
  { id: "about", label: "About" },
];

export function SidebarNav({ active, onChange, providers, paths, voiceEnabled }: Props) {
  const libraryBack = useLibraryBack();
  const { group, pill } = useNavPill(active, { inset: "inset-x-3" });
  // Local-only holds iff no active provider points to a non-localhost host.
  const localOnly = !!providers && !providers.some(
    (p) => p.is_active && !/localhost|127\.0\.0\.1/.test(p.base_url),
  );

  const badge = slots.useSidebarBadge();

  return (
    <nav
      ref={group}
      className="vt-settings-nav relative w-53 shrink-0 bg-paper border-r border-line flex flex-col"
      aria-label="Settings sections"
    >
      <header className="px-5 pt-6 pb-2 space-y-3">
        <Link
          {...libraryBack}
          className="inline-flex items-center gap-1.5 text-ui font-medium text-mut hover:text-ink transition-colors"
          aria-label="Back to library"
        >
          <ArrowLeft size={16} aria-hidden />
          <span>Library</span>
        </Link>
        <h1 className="heading-wordmark">
          Settings
        </h1>
      </header>
      {pill}
      <ul className="flex-1 py-4 px-3 space-y-1">
        {SECTIONS.map((s) => (
          <li key={s.id}>
            <button
              type="button"
              onClick={() => onChange(s.id)}
              aria-current={active === s.id ? "page" : undefined}
              className={`w-full text-left px-3 py-2 rounded-button text-ui ${active === s.id ? navRow("pill") : `${navRow("idle")} font-medium`}`}
            >
              {s.label}
              {s.id === "voice" && voiceEnabled === false && <NewPill active={active === s.id} />}
              {s.Badge && <s.Badge active={active === s.id} />}
            </button>
          </li>
        ))}
      </ul>
      <footer className="p-4 border-t border-line space-y-2">
        {(badge || localOnly) && (
          <span data-testid={badge ? "sidebar-badge" : "local-only-pill"} className="inline-block">
            <Pill tone="matcha" className="text-ink!">
              <span className="w-1.5 h-1.5 rounded-pill bg-matcha" />
              {badge ?? "Local-only · on"}
            </Pill>
          </span>
        )}
        {paths ? (
          <div className="text-ui text-mut leading-relaxed">
            keys → <PathCode path={paths.keys} />
            <br />
            data → <PathCode path={`${paths.data_dir}/`} />
          </div>
        ) : (
          <div aria-hidden className="text-ui leading-relaxed">
            <div className="h-5.25 flex items-center"><Shimmer className="h-2.25 w-[70%]" /></div>
            <div className="h-5.25 flex items-center"><Shimmer className="h-2.25 w-[58%]" /></div>
          </div>
        )}
      </footer>
    </nav>
  );
}

/** Wraps a long path after a `/` rather than mid-name. `<wbr>` keeps a copied path clean. */
function PathCode({ path }: { path: string }) {
  return (
    <code className="font-mono [overflow-wrap:anywhere]">
      {path.split("/").map((seg, i) => (
        <Fragment key={i}>
          {i > 0 && (
            <>
              /<wbr />
            </>
          )}
          {seg}
        </Fragment>
      ))}
    </code>
  );
}
