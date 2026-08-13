# CKB × EchoForge Intelligence Bridge v1

CKB owns **code/architecture intelligence**. EchoForge Sentinel owns **runtime/incident intelligence**. The bridge correlates the two without merging their codebases or trust domains.

Bridge version: `ckb-echoforge/v1`

## Runtime → CKB enrichment

EchoForge stores an explicit `EchoForge Sentinel project ↔ CKB repo_name` mapping. Incident telemetry can identify affected source using `file`, `path`, `file_path`, `line`, `symbol`, and commit metadata.

Before impact analysis EchoForge must verify the mapped named CKB session exists through the repo-scoped report endpoint:

`GET /api/v1/report?repo=<owner/repository>`

Only after that verification may it call:

```json
POST /api/v1/impact
{
  "path": "__ckb_bridge_requires_existing_repo_session__",
  "repo_name": "owner/repository",
  "file": "src/checkout.ts",
  "line": 84,
  "change_type": "modify"
}
```

The placeholder path is deliberately non-resolvable. The current CKB handler requires a `path` field for its pre-scan fallback, but the bridge must never use that fallback for a remotely mapped customer project. If the named session disappears between report verification and impact analysis, the request fails safely instead of scanning an unrelated server filesystem location.

Repo-scoped auxiliary evidence:

- `GET /api/v1/test-gaps?repo=<owner/repository>`
- architecture drift from the mapped `/api/v1/report` response

Do not attach the current `/api/v1/drift-timeline` or `/api/v1/metrics/intelligence` output to one customer project: those routes are not currently single-repo scoped.

## CKB → EchoForge runtime evidence

CKB publishes architecture observations using a dedicated Sentinel project ingestion key and EchoForge's existing ingestion contract:

```http
POST /api/sentinel/ingest
X-Sentinel-Key: efp_live_...
Content-Type: application/json
```

```json
{
  "domain": "infrastructure",
  "context": {
    "source": "ckb",
    "sourceIncidentId": "ckb:snapshot:owner/repo:commit",
    "repo": "owner/repo",
    "commit_sha": "commit",
    "bridge_protocol": "ckb-echoforge/v1"
  },
  "signals": [
    {
      "name": "ckb.failure_risk",
      "score": 0.91,
      "value": 0.91,
      "detector": "ckb",
      "entity": "owner/repo",
      "service": "ckb-architecture",
      "metadata": { "file": "src/checkout.ts", "line": 84 }
    }
  ]
}
```

Recommended signals:

- `ckb.failure_risk`
- `ckb.blast_radius`
- `ckb.architecture_drift`
- `ckb.test_gap`
- `ckb.hotpath_latency`
- `ckb.semantic_clone_risk`

## Identity and tenancy

`EchoForge project ID ↔ CKB repo_name` is an explicit mapping. Neither side may infer another tenant's project from an untrusted repository name. Service credentials are server-only and scoped independently.

External source identities are bounded by EchoForge and prefixed by the authenticated Sentinel project before persistence. A stable `ckb:snapshot:<repo>:<commit>` identity therefore updates the same project-scoped incident row on replay.

## Safety

The bridge is evidence-only by default. CKB may recommend a code change and EchoForge may recommend an operational response, but the bridge MUST NOT silently commit code, deploy, restart services, block identities, hold payments, stop machines, alter privileges, or reroute AI. Those actions remain inside each product's existing approval/audit workflow.

## Failure behavior

Bridge failure is non-fatal to either product. EchoForge persists the incident before optional CKB enrichment. CKB keeps providing architecture intelligence if EchoForge is unavailable. Bridge responses expose `status: enriched | partial | unavailable`.

## Closed-loop lifecycle

`Code graph → runtime signal → Sentinel incident → CKB enrichment → root cause/blast radius → guarded fix → deployment → EchoForge verification → Failure Memory`
