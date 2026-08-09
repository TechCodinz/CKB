# CKB Architecture Intelligence V2

## Goal
Turn CKB from a collection of analysis outputs into an evidence-backed software intelligence engine. Every visual or prediction must declare whether it is **static**, **runtime-observed**, or **predicted**, and must carry provenance/confidence rather than presenting simulations as facts.

## Cross-repository contract
`CKB` is the source-of-truth analysis engine. `ckb-cloud` is the authenticated SaaS, ingestion/orchestration and visualization surface. Cloud must consume normalized CKB intelligence rather than reimplementing weaker regex-only semantics for features that CKB can compute.

## Intelligence layers

### 1. Static structural intelligence
- AST nodes with stable IDs, source spans and symbol identity.
- Import, call, inheritance, implementation, return, parameter and property edges.
- Architectural boundaries and drift violations.
- Transitive blast radius with shortest paths and edge reasons.
- Test coverage and contract impact.

### 2. Runtime intelligence
- OTLP traces mapped to static nodes by code attributes, service/module identity and source location.
- Per-node and per-edge invocation count, latency distribution, error rate and last-seen timestamp.
- Runtime paths are never inferred solely from animation.
- Unmapped telemetry is retained and explicitly labelled `unmapped`.

### 3. Predictive intelligence
- What-if change scenarios operate on a copy/overlay of the graph.
- Predictions include evidence, confidence, assumptions and affected paths.
- No fixed percentages or fabricated future dates.

## Normalized evidence model
Every intelligence datum exposed to UI/agents should be representable as:

```json
{
  "kind": "static|runtime|predicted",
  "confidence": 0.0,
  "evidence": [{"source":"ast|git|otlp|test|contract", "ref":"..."}],
  "observedAt": null,
  "explanation": "human-readable reason"
}
```

## Required API surfaces
- `GET /intelligence/graph` — normalized static graph.
- `POST /intelligence/impact` — transitive what-if blast radius.
- `POST /intelligence/telemetry/otlp` — ingest runtime observations.
- `GET /intelligence/runtime` — node/edge runtime overlays.
- `GET /intelligence/source/:nodeId` — source span + line-level evidence.
- `GET /intelligence/history` — real Git-derived architecture snapshots/deltas.

Names may differ at implementation level, but the semantics above are the contract.

## UI rules
- Cyan/blue: static structural relationship.
- Green: runtime-observed healthy path.
- Amber/red: runtime-observed degradation or evidence-backed violation.
- Purple/dashed: prediction/simulation.
- Never label a mock/fallback animation as live execution.
- A user can inspect any visual edge and see its evidence and confidence.

## Priority implementation sequence
1. Stable symbol IDs + source spans.
2. Correct cross-file call/import resolution.
3. Full transitive impact paths (not only two hops).
4. OTLP mapping and edge-level runtime traces.
5. Cloud API contract and real graph overlays.
6. Monaco line decorations backed by source spans.
7. Git-history snapshots/deltas.
8. Evidence-backed what-if simulator.
9. Only then predictive maintenance/refactoring models.

## Quality gates
- No hard-coded demo metrics in production intelligence views.
- No silent mock graph when an API request fails; expose an explicit demo mode/error state.
- No claim of `live`, `real-time`, `AI predicted`, or `self-healing` without corresponding engine evidence.
- Unit tests for graph traversal and telemetry mapping.
- Contract tests between CKB core/MCP and ckb-cloud.
- Large-repository performance benchmarks before enabling expensive visual layers by default.
