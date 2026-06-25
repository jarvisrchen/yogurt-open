import { Link, Outlet, useLocation, useNavigate } from "react-router";
import { Settings as SettingsIcon } from "lucide-react";
import { Logo } from "../Logo";
import { useQuery } from "@tanstack/react-query";
import { settingsApi } from "../../lib/api/settings";
import { Segmented } from "../Segmented";
import { VoiceNav } from "../voice/VoiceNav";
import { LibraryNav } from "./LibraryNav";
import { slots } from "../../slots";

/** Every page with the app sidebar renders under this one, so the sidebar stays mounted between them and keeps its pill and open groups. */
export function WithSidebar() {
  return (
    <div className="flex">
      <Sidebar />
      <Outlet />
    </div>
  );
}

export function Sidebar() {
  const navigate = useNavigate();
  const inVoice = useLocation().pathname.startsWith("/voice");
  const settings = useQuery({
    queryKey: ["settings"],
    queryFn: () => settingsApi.get(),
    staleTime: 30_000,
  });

  const badge = slots.useSidebarBadge();
  const isLocalOnly =
    !settings.data?.providers?.some(
      (p) => p.is_active && !isLocalProvider(p.base_url),
    );


  return (
    <aside
      // Sticky keeps the rail at viewport height so `mt-auto` parks Settings at
      // the bottom of the screen, not the bottom of a long meeting list.
      className="vt-sidebar w-53 shrink-0 flex flex-col bg-paper border-r border-line sticky top-0 h-screen"
    >
      <div className="px-5 pt-5 pb-3 flex items-center gap-2">
        <Logo size={28} />
        <span className="heading-wordmark">
          yogurt
        </span>
      </div>

      <div className="px-4 pb-3">
        <Segmented
          label="Library"
          fill
          options={[
            { value: "meetings", label: "Meetings" },
            { value: "voice", label: "Voice" },
          ]}
          value={inVoice ? "voice" : "meetings"}
          onChange={(v) => navigate(v === "voice" ? "/voice" : "/")}
        />
      </div>

      {inVoice ? (
        <VoiceNav />
      ) : (
        <LibraryNav />
      )}

      <div className="mt-auto px-4 py-4 flex flex-col gap-2">
        {(badge || isLocalOnly) && (
          <div
            className="self-start px-2.5 py-1 rounded-pill bg-mtsoft text-ink text-caption font-medium"
          >
            {badge ?? "Local-only · on"}
          </div>
        )}
        <Link
          to="/settings"
          className="flex items-center gap-2 px-2 py-1.5 rounded-button text-ui text-ink hover:bg-line/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue/40"
        >
          <SettingsIcon size={16} aria-hidden />
          <span>Settings</span>
        </Link>
      </div>
    </aside>
  );
}

// The settings shape has no local/cloud discriminator, so infer it from the URL.
function isLocalProvider(baseUrl: string): boolean {
  const u = baseUrl.toLowerCase();
  return (
    u.includes("localhost") ||
    u.includes("127.0.0.1") ||
    u.includes("0.0.0.0") ||
    u.includes(":11434") ||
    u.includes(":1234")
  );
}
