import {
  createBrowserRouter,
  Navigate,
  Outlet,
  redirect,
  ScrollRestoration,
  type RouteObject,
} from "react-router";
import { useEffect } from "react";
import { Library } from "./routes/Library";
import { WithSidebar } from "./components/library/Sidebar";
import { useFirstRunRedirect } from "./hooks/useFirstRunRedirect";
import { RecordingPill } from "./components/RecordingPill";
import { MeetingDetectedBanner } from "./components/MeetingDetectedBanner";
import { DownloadPill } from "./components/DownloadPill";
import { ImportPill } from "./components/ImportPill";
import { UpdateToast } from "./components/UpdateToast";
import { ModelDownloadDialog } from "./components/dialogs/ModelDownloadDialog";
import { useModelDownloadEvents } from "./hooks/useModelDownload";
import { animateByDefault } from "./lib/viewTransition";
import { Slot, slots } from "./slots";

// Every page but the Library loads on demand, so a cold start evaluates only what the Library needs (the editor alone is most of the bundle).
const pages = {
  meeting: () => import("./routes/Meeting").then((m) => ({ Component: m.Meeting })),
  meetingPost: () => import("./routes/MeetingPost").then((m) => ({ Component: m.MeetingPost })),
  settings: () => import("./routes/Settings").then((m) => ({ Component: m.Settings })),
  voice: () => import("./routes/Voice").then((m) => ({ Component: m.Voice })),
  welcome: () => import("./routes/Welcome").then((m) => ({ Component: m.Welcome })),
  styleGuide: () => import("./routes/StyleGuide").then((m) => ({ Component: m.StyleGuide })),
};

function usePrefetchPages() {
  useEffect(() => {
    const t = setTimeout(() => {
      void pages.meetingPost();
      void pages.meeting();
      void pages.settings();
      void pages.voice();
    }, 1_000);
    return () => clearTimeout(t);
  }, []);
}

// Pathless Shell wraps every route. useFirstRunRedirect only gates `/`, so deep links work before onboarding.
// The banner and recording pill can never show together: the server reports no detection while a recording is live.
// The download, import, and push pills can sit beside either, so they all share one centered row.
function Shell() {
  useFirstRunRedirect();
  useModelDownloadEvents();
  usePrefetchPages();
  return (
    <>
      <div className="fixed top-4 left-[calc((var(--dock-vw,100vw)-var(--dock-clear,0px))/2)] -translate-x-1/2 flex items-center gap-2 z-30">
        <RecordingPill />
        <MeetingDetectedBanner />
        <DownloadPill />
        <ImportPill />
        <Slot of={slots.shellPills} />
      </div>
      <ModelDownloadDialog />
      <UpdateToast />
      <ScrollRestoration />
      <Outlet />
    </>
  );
}

export const routes: RouteObject[] = [
  {
    element: <Shell />,
    // Blank on purpose: a cold load onto a lazy page waits only for its code.
    HydrateFallback: () => null,
    children: [
      {
        element: <WithSidebar />,
        children: [
          { path: "/", element: <Library /> },
          { path: "/starred", element: <Library starredOnly /> },
          { path: "/label/:labelId", element: <Library /> },
          { path: "/voice", lazy: pages.voice },
          { path: "/voice/hands-free", lazy: pages.voice },
          ...slots.routes,
        ],
      },
      { path: "/welcome", lazy: pages.welcome },
      { path: "/style-guide", lazy: pages.styleGuide },
      { path: "/meeting/new", lazy: pages.meeting },
      { path: "/meeting/:id", lazy: pages.meeting },
      { path: "/meeting/:id/post", lazy: pages.meetingPost },
      // A loader redirect resolves before anything renders, so a page transition lands on Transcription rather than on an empty redirect.
      { path: "/settings", loader: () => redirect("/settings/transcription") },
      { path: "/settings/:section/:view?", lazy: pages.settings },
      { path: "*", element: <Navigate to="/" replace /> },
    ],
  },
];

export const router = createBrowserRouter(routes);

// React Router has no app-wide default, and every Link and navigate() goes through here.
const navigate = router.navigate.bind(router);
router.navigate = ((to, opts) => navigate(to as never, animateByDefault(to, opts))) as typeof router.navigate;
