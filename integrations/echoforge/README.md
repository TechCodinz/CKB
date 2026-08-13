# EchoForge Sentinel integration

CKB and EchoForge remain independently deployable products connected by the versioned `ckb-echoforge/v1` bridge. CKB supplies source/architecture evidence; EchoForge Sentinel supplies runtime incident correlation, BlackBox reconstruction, Failure Memory and guarded response verification.

## Trust boundary

Keep every bridge credential server-side. Do **not** expose CKB or EchoForge project keys in a VS Code webview, browser bundle, logs, generated reports, or repository files.

The bridge carries evidence and recommendations only. It does not silently commit, merge, deploy, restart services, block identities, stop machines or alter payments.

## CKB → EchoForge signals

The TypeScript adapter in `bridge.ts` publishes normalized architecture evidence to EchoForge's existing Sentinel project ingestion contract using `x-sentinel-key`.

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

Supported bridge signal families include failure risk, blast radius, architecture drift, test gaps, hotpath latency and semantic-clone risk.

## Operational snapshot publisher

`publish-snapshot.mjs` is a ready-to-run server-side worker for CI post-scan hooks, private cron workers, or deployment pipelines. It reads the mapped repository's **repo-scoped** CKB report/test-gap APIs and sends one stable snapshot to EchoForge Sentinel.

Required environment:

```bash
CKB_BRIDGE_SOURCE_URL=https://your-ckb-mcp-server.example.com
CKB_BRIDGE_SOURCE_API_KEY=...
CKB_REPO_NAME=owner/repository
CKB_REPO_COMMIT=$(git rev-parse HEAD)

ECHOFORGE_SENTINEL_URL=https://app.echoforge.com
ECHOFORGE_SENTINEL_PROJECT_KEY=efp_live_...
CKB_ECHOFORGE_TIMEOUT_MS=5000
```

Before publishing, the repository must have a named CKB session. Scan with the same repository identity:

```http
POST /api/v1/scan
X-API-Key: <ckb-key>
Content-Type: application/json

{
  "path": "/absolute/path/to/repository",
  "repo_name": "owner/repository"
}
```

Then publish:

```bash
node integrations/echoforge/publish-snapshot.mjs
```

The publisher uses a stable source identity of `ckb:snapshot:<repo>:<commit>` so EchoForge can upsert the same project-scoped incident instead of creating another database row for the same snapshot.

## Tenant-scope rule

The integration deliberately does **not** use CKB's current `/api/v1/metrics/intelligence` or `/api/v1/drift-timeline` routes for a single Sentinel project:

- intelligence metrics currently aggregate the federated server view;
- drift timeline currently reads the CKB server working directory and is not repo/session scoped.

Instead, the bridge uses `/api/v1/report?repo=...` and `/api/v1/test-gaps?repo=...`, and derives architectural drift from that mapped repository's scan report. This prevents another repository's evidence from leaking into a customer's BlackBox.

## EchoForge → CKB

EchoForge's server-side bridge first verifies that the explicitly mapped repo has an active named CKB scan session. Only then does it call `/api/v1/impact` for affected files. If the named session is missing, enrichment fails safely rather than asking CKB to scan an unrelated server filesystem path.

CKB downtime is non-fatal to EchoForge incidents, and EchoForge downtime is non-fatal to normal CKB architecture analysis.

## Closed loop

`CKB code graph → runtime signal → Sentinel incident → CKB blast-radius/test-gap/drift evidence → guarded remediation → deployment → EchoForge verification → Failure Memory`
