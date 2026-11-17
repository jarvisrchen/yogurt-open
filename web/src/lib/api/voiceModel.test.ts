import { describe, expect, it } from "vitest";
import { voiceModel } from "./settings";
import type { ModelName } from "../../components/settings/modelTypes";

const g = (voice_stt_model: string) => ({ voice_stt_model, stt_model: "small.en" });
const on = (...names: ModelName[]) =>
  (["parakeet-ultra", "parakeet-v3"] as const).map((name) => ({ name, downloaded: names.includes(name) }));

describe("voiceModel", () => {
  it("resolves the default against the downloaded Parakeets, as the server does", () => {
    expect(voiceModel(g(""), on("parakeet-v3"))).toBe("parakeet-v3");
    expect(voiceModel(g(""), on("parakeet-ultra", "parakeet-v3"))).toBe("parakeet-ultra");
    expect(voiceModel(g(""), on("parakeet-ultra"))).toBe("parakeet-ultra");
    expect(voiceModel(g(""), on())).toBe("parakeet-ultra");
  });

  it("is unknown on the default until the models list loads", () => {
    expect(voiceModel(g(""), undefined)).toBeUndefined();
  });

  it("keeps a picked model and follows meetings, whatever is downloaded", () => {
    expect(voiceModel(g("parakeet-v3"), on("parakeet-ultra"))).toBe("parakeet-v3");
    expect(voiceModel(g("meetings"), undefined)).toBe("small.en");
  });
});
