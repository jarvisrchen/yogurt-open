import { useEffect, useState } from "react";

/** Now, refreshed just after each local midnight, so stats summed on a page left open move on to the new day. */
export function useDayNow(): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const d = new Date(now);
    const midnight = new Date(d.getFullYear(), d.getMonth(), d.getDate() + 1).getTime();
    const timer = setTimeout(() => setNow(Date.now()), midnight - now + 1000);
    return () => clearTimeout(timer);
  }, [now]);
  return now;
}
