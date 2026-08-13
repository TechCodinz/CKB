# CKB × EchoForge Intelligence Bridge v1

CKB owns **code/architecture intelligence**. EchoForge Sentinel owns **runtime/incident intelligence**. The bridge correlates the two without merging their codebases or trust domains.

## Contract

Bridge version: `ckb-echoforge/v1`

### Runtime → CKB enrichment request

```json
{
  "protocol": "ckb-echoforge/v1",
  "project": { "external_id": "project-id", "repo_name": "owner/repo", "commit_sha": "optional" },
  "incident": { "id": "incident-id", "title": "Checkout failures", "severity": "critical", "timestamp": "ISO-8601" },
  "evidence": {
    "files": [{ "path": "src/checkout.ts", "line": 84 }],
    "services": ["checkout-api"],
    "trace_ids": ["..."],
    "deployment_ids": ["..."]
  }
}
```

CKB enriches the incident with blast radius, architecture context, failure-risk scores, test gaps, drift evidence and affected code. EchoForge persists that output as incident evidence/BlackBox context.

### CKB → EchoForge runtime evidence

CKB can publish architecture observations as Sentinel telemetry using a dedicated project ingestion key. Recommended signals:

- `ckb.failure_risk`
- `ckb.blast_radius`
- `ckb.architecture_drift`
- `ckb.test_gap`
- `ckb.hotpath_latency`
- `ckb.semantic_clone_risk`

Every signal SHOULD include `repo`, `commit_sha`, `file`, `line`, `symbol`, and `ckb_scan_id` when known.

## Identity and tenancy

`EchoForge project ID ↔ CKB repo_name` is an explicit mapping. Neither side may infer another tenant's project from a repository name supplied by an untrusted caller. Service credentials are server-only and scoped independently.

## Safety

The bridge is evidence-only by default. CKB may recommend a code change and EchoForge may recommend an operational response, but the bridge MUST NOT silently commit code, deploy, restart services, block identities, stop machinery, or alter payments. Those actions remain inside each product's existing approval/audit workflow.

## Idempotency

Use `incident.id + repo_name + commit_sha` as the enrichment idempotency tuple. Replays update existing evidence rather than create duplicate incidents.

## Failure behavior

Bridge failure is non-fatal to either product. EchoForge must still retain the incident if CKB is unavailable. CKB must still provide architecture intelligence if EchoForge is unavailable. Bridge responses expose `status: enriched | partial | unavailable`.

## Closed-loop lifecycle

`Code graph → runtime signal → Sentinel incident → CKB enrichment → root cause/blast radius → guarded fix → deployment → EchoForge verification → Failure Memory`
