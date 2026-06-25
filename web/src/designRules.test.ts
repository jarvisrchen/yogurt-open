import { describe, expect, it } from "vitest";

const sources = import.meta.glob(["./**/*.{ts,tsx,css}", "!./**/*.test.*"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const offenders = (pattern: RegExp, keep: (match: string) => boolean = () => true) =>
  Object.entries(sources).flatMap(([file, src]) =>
    [...src.matchAll(pattern)].map((m) => m[0]).filter(keep).map((m) => `${file}: ${m}`),
  );

describe("layout", () => {
  // A page that rendered its own copy would remount the sidebar on every hop, so its pill would jump and its open groups reset.
  it("renders the app sidebar only from the WithSidebar layout route", () => {
    expect(offenders(/import\s*\{[^}]*\bSidebar\b[^}]*\}\s*from\s*["'][^"']*\/Sidebar["']/g)).toEqual([]);
  });
});

describe("typography", () => {
  it("never sets a label in mono, which was the pre-Manrope eyebrow", () => {
    const classStrings = /["'`][^"'`\n]*\bfont-mono\b[^"'`\n]*["'`]/g;
    expect(offenders(classStrings, (s) => /\buppercase\b|tracking-\[/.test(s))).toEqual([]);
  });

  it("picks families only through the font-sans and font-mono tokens", () => {
    expect(offenders(/fontFamily\s*:|\bfont-\[|font-serif|@fontsource\/(?!manrope|jetbrains-mono)[\w-]+/g)).toEqual([]);
  });
});
