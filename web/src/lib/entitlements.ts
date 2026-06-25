import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { HttpError } from "./httpError";
import { bearerFetch, bearerJson } from "./session";

export interface LicenseEntitlement {
  name: string;
  /** `"lifetime"` or `YYYY-MM-DD` for a purchase, `null` for a grant. */
  until: string | null;
  /** `YYYY-MM-DD` for a grant, `null` for a purchase. */
  expires: string | null;
  /** Whether it holds in this build today. */
  active: boolean;
}

export interface License {
  id: string;
  email: string;
  issued: string;
  founder: number | null;
  build_date: string;
  entitlements: LicenseEntitlement[];
}

export const LICENSE_KEY = ["license"] as const;

/** `null` when no license is stored, and on a build without Pro, which has no license route. */
export async function fetchLicense(): Promise<License | null> {
  const res = await bearerFetch("/api/license");
  if (res.status === 404) return null;
  if (!res.ok) throw new HttpError(res.status, `license: ${res.status} ${res.statusText}`);
  return ((await res.json()) as { license: License | null }).license;
}

export function useLicense() {
  return useQuery({ queryKey: LICENSE_KEY, queryFn: fetchLicense, retry: false });
}

export function proOf(license: License | null | undefined): LicenseEntitlement | undefined {
  return license?.entitlements.find((e) => e.name === "pro");
}

export function hasPro(license: License | null | undefined): boolean {
  return proOf(license)?.active ?? false;
}

/** Today as a local `YYYY-MM-DD`, the form a license's dates take. */
export function today(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/** A yearly license's updates date has passed. Builds dated after it run without Pro. */
export function updatesEnded(e: LicenseEntitlement | undefined, now = today()): boolean {
  return !!e?.until && e.until !== "lifetime" && now > e.until;
}

/** Pro holds in this build, and a version released today would turn it off. */
export function updateEndsPro(license: License | null | undefined, now = today()): boolean {
  const pro = proOf(license);
  return !!pro?.active && updatesEnded(pro, now);
}

/** What the stored license unlocks in this build. `pro` stays false while loading and on any error. */
export function useEntitlements(): { pro: boolean; loading: boolean } {
  const { data, isLoading } = useLicense();
  return {
    pro: hasPro(data),
    loading: isLoading,
  };
}

/**
 * `gcTime: 0` drops the license text from the mutation cache once nothing observes it.
 * Every query refetches after, since Pro changes what the meeting and label routes return.
 */
function useLicenseMutation<V>(fn: (v: V) => Promise<unknown>) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: fn,
    gcTime: 0,
    onSettled: () => qc.invalidateQueries(),
  });
}

export function useAddLicense() {
  return useLicenseMutation((license: string) =>
    bearerJson<{ license: License }>("/api/license", { method: "POST", body: JSON.stringify({ license }) }),
  );
}

export function useRemoveLicense() {
  return useLicenseMutation(() => bearerJson<void>("/api/license", { method: "DELETE" }));
}
