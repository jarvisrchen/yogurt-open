import { useContext } from "react";
import { matchPath, UNSAFE_ViewTransitionContext, type NavigateOptions, type To } from "react-router";

// Search, paging, redirects, and history steps change the URL without leaving the page, so they stay still.
export function animateByDefault(to: To | number | null, opts?: NavigateOptions): NavigateOptions | undefined {
  if (opts?.viewTransition !== undefined || opts?.replace) return opts;
  if (to === null || typeof to === "number") return opts;
  const searchOnly = typeof to === "string" ? to.startsWith("?") || to.startsWith("#") : !to.pathname;
  return searchOnly ? opts : { ...opts, viewTransition: true };
}

// React Router's useViewTransitionState answers this too, but throws outside a data router, where tests render these components.
export function useTransitionTouches(pattern: string): boolean {
  const vt = useContext(UNSAFE_ViewTransitionContext);
  if (!vt.isTransitioning) return false;
  return matchPath(pattern, vt.currentLocation.pathname) != null || matchPath(pattern, vt.nextLocation.pathname) != null;
}
