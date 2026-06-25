import { describe, expect, it } from "vitest";
import { animateByDefault } from "./viewTransition";

describe("animateByDefault", () => {
  it("animates a move to another page", () => {
    expect(animateByDefault("/settings/model")).toEqual({ viewTransition: true });
    expect(animateByDefault({ pathname: "/voice" }, { state: { x: 1 } })).toEqual({ state: { x: 1 }, viewTransition: true });
  });

  it("leaves search, replace, and history steps alone", () => {
    expect(animateByDefault("?q=standup")).toBeUndefined();
    expect(animateByDefault({ search: "?page=2" })).toBeUndefined();
    expect(animateByDefault("/", { replace: true })).toEqual({ replace: true });
    expect(animateByDefault(-1)).toBeUndefined();
  });

  it("keeps an explicit choice", () => {
    expect(animateByDefault("/", { viewTransition: false })).toEqual({ viewTransition: false });
  });
});
