import { useMutation } from "@tanstack/react-query";
import { settingsApi, type TestConnectionResult } from "../../lib/api/settings";
import { Button } from "../Button";

// Renders a fragment: the button plus a `basis-full order-last` verdict line, so a
// `flex flex-wrap` parent drops the verdict onto its own last row, even with siblings after the button.
export function TestKeyButton({
  providerId = "",
  providerName,
  // Unsaved key from the password field. Empty tests the stored key.
  draft = "",
  // The model and base URL in effect, unsaved edits included, so Test tries them before Save.
  config,
  hasStoredKey,
  // A `cli` provider has no key, so Test is always reachable.
  alwaysTestable = false,
  // Override for non-provider callers such as the Deepgram STT key.
  testFn = (key?: string) => settingsApi.testProvider(providerId, key, config),
  onResult,
  // `false` beside the API key input keeps `Button`'s default sizing to line up with Save key.
  compact = true,
  // Matches a `sm` Save key beside it. Overrides `compact`.
  size,
}: {
  providerId?: string;
  providerName: string;
  draft?: string;
  config?: { model?: string; base_url?: string };
  hasStoredKey?: boolean;
  alwaysTestable?: boolean;
  testFn?: (key?: string) => Promise<TestConnectionResult>;
  onResult?: (result: TestConnectionResult) => void;
  compact?: boolean;
  size?: "sm";
}) {
  const test = useMutation({
    mutationFn: ({ key }: { key: string; config: string }) => testFn(key || undefined),
    onSuccess: (result) => onResult?.(result),
  });

  // Hide the verdict once the key or config changes, so a green tick never sits beside an untested setup.
  const configKey = JSON.stringify(config ?? {});
  const fresh = test.variables?.key === draft && test.variables.config === configKey;
  const canTest = (alwaysTestable || !!draft || !!hasStoredKey) && !test.isPending;

  return (
    <>
      <Button
        variant="secondary"
        disabled={!canTest}
        aria-label={`Test connection for ${providerName}`}
        size={size}
        className={`shrink-0 ${compact && !size ? "px-3 py-1.5" : ""}`}
        onClick={() => test.mutate({ key: draft, config: configKey })}
      >
        {test.isPending ? "Testing…" : "Test"}
      </Button>

      {fresh && test.data && (
        <p
          role="status"
          className={`order-last basis-full text-meta ${
            test.data.ok ? "text-matcha" : "text-straw"
          }`}
        >
          {test.data.ok
            ? `✓ Connection works${test.data.model ? ` · answered as ${test.data.model}` : ""}`
            : `✗ ${test.data.error ?? "Connection failed."}`}
        </p>
      )}
      {fresh && test.isError && (
        <p role="status" className="order-last basis-full text-meta text-straw">
          ✗ Could not reach the yogurt server to run the test.
        </p>
      )}
    </>
  );
}
