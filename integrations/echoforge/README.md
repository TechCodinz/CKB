# EchoForge Sentinel integration

This package is the CKB-side adapter for the CKB × EchoForge Intelligence Bridge.

## Environment

- `ECHOFORGE_SENTINEL_URL` — EchoForge web/API origin.
- `ECHOFORGE_SENTINEL_PROJECT_KEY` — a one-time-display `efp_live_*` or `efp_test_*` key created for the mapped Sentinel project.
- `ECHOFORGE_SENTINEL_TIMEOUT_MS` — optional request timeout; default 5000 ms.

Keep the project key server-side. Do not expose it in a VS Code webview, browser bundle, logs, generated reports, or repository files.

## Usage

```ts
import { EchoForgeBridgeClient } from "./bridge";

const bridge = new EchoForgeBridgeClient({
  baseUrl: process.env.ECHOFORGE_SENTINEL_URL!,
  projectKey: process.env.ECHOFORGE_SENTINEL_PROJECT_KEY!,
});

await bridge.publishArchitectureSignals({
  sourceIncidentId: `ckb:${repo}:${commit}`,
  repoName: repo,
  commitSha: commit,
  signals: [
    {
      name: "ckb.failure_risk",
      value: 0.91,
      severity: "high",
      attributes: { file: "src/checkout.ts", line: 84 },
    },
  ],
});
```

The adapter is deliberately fail-independent: callers should catch bridge failures and continue normal CKB analysis. EchoForge integration enriches CKB; it must never become a prerequisite for local architecture intelligence.
