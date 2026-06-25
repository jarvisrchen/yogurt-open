import { Fragment, type ReactNode } from "react";
import { House } from "lucide-react";
import { Link, useLocation, useSearchParams } from "react-router";
import { useSettings } from "../../lib/api/settings";
import { useEntitlements } from "../../lib/entitlements";
import { useVoiceHistory } from "../../lib/api/voice";
import { formatCount } from "../library/pagination";
import { LABEL } from "../settings/labelClass";
import { hotkeyKeys } from "../../lib/hotkey";
import { navRow, useNavPill } from "../NavPill";
import { Slot, slots } from "../../slots";

const ROW = "px-3 py-1.5 rounded-button text-ui flex items-center gap-1.5";
const KEYCAP =
  "inline-flex items-center justify-center min-w-5.5 h-5 px-1.5 rounded-[6px] border border-line bg-card text-caption font-semibold font-mono text-ink shadow-[0_1px_0_var(--color-line)] align-middle";

export function VoiceNav() {
  const { pathname } = useLocation();
  const [params] = useSearchParams();
  const app = params.get("app");
  const history = useVoiceHistory()?.history;
  const general = useSettings().data?.general;
  const { pro } = useEntitlements();
  const handsFree = pathname === "/voice/hands-free";
  const { group, pill } = useNavPill(pathname + (app ?? ""), {
    inset: "inset-x-2",
    // The slot's rows arrive with the license, which moves every row under them.
    labels: `${pro}\n${history?.apps.map((a) => a.name).join("\n")}`,
  });

  return (
    <>
      <div className="px-4 pb-3">
        <p className="rounded-button border border-line bg-card px-3 py-2.5 text-meta text-mut leading-relaxed">
          {general && !general.voice_enabled ? (
            <>
              Voice is off. Turn it on in{" "}
              <Link to="/settings/voice" className="text-blue font-semibold hover:underline">
                Settings
              </Link>
              .
            </>
          ) : (
            <>
              Hold or tap{" "}
              {hotkeyKeys(general?.voice_hotkey ?? "fn").map((k, i) => (
                <Fragment key={i}>
                  {i > 0 && "+"}
                  <kbd className={KEYCAP}>{k}</kbd>
                </Fragment>
              ))}{" "}
              in any app to dictate.
            </>
          )}
        </p>
      </div>

      <nav ref={group} className="relative">
        {pill}
        <div className="px-2 flex flex-col gap-0.5">
          <Row
            to="/voice"
            active={pathname === "/voice" && !app}
            icon={<House size={14} className="shrink-0" aria-hidden />}
            label="Home"
            count={history?.count}
          />
          <Slot of={slots.voiceSidebar} />
        </div>

        <div className={`relative px-5 pt-5 pb-1 ${LABEL}`}>History</div>
        <div className="px-2 flex flex-col gap-0.5">
          <Row to="/voice/hands-free" active={handsFree} label="Hands-free" count={history?.hands_free_count} />
        </div>

        <div className={`relative px-5 pt-5 pb-1 ${LABEL}`}>Apps</div>
        <div className="px-2 flex flex-col gap-0.5">
          {history && history.apps.length === 0 ? (
            <p className="px-3 py-1 text-meta text-mut">No saved dictations</p>
          ) : (
            history?.apps.map((a) => (
              <Row
                key={a.name}
                to={`/voice?${new URLSearchParams({ app: a.name })}`}
                active={app === a.name}
                label={a.name}
                count={a.count}
              />
            ))
          )}
        </div>
      </nav>
    </>
  );
}

export function Row({
  to,
  active,
  icon,
  label,
  count,
}: {
  to: string;
  active: boolean;
  icon?: ReactNode;
  label: string;
  count?: number;
}) {
  return (
    <Link
      to={to}
      aria-current={active ? "page" : undefined}
      className={`${ROW} ${navRow(active ? "pill" : "idle")}`}
    >
      {icon}
      <span className="flex-1 truncate">{label}</span>
      {count != null && <span className="text-caption text-mut tabular-nums font-normal">{formatCount(count)}</span>}
    </Link>
  );
}
