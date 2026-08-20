# CKB Live Execution Twin — Software MRI

The Live Execution Twin is CKB's runtime architecture intelligence layer. Its job is to let a developer watch **observed software execution** move through source-resolved functions, services, databases, caches, queues, events, WebSockets and external calls without turning a static architecture graph into decorative fake traffic.

## Core promise

CKB separates three realities:

1. **STATIC** — what source analysis establishes from the repository.
2. **OBSERVED** — what runtime telemetry proves executed.
3. **PREDICTED** — what CKB analysis estimates may happen after a proposed change.

A visual pulse, execution path or runtime causal statement belongs in **OBSERVED** only when exact runtime evidence exists.

## Runtime evidence path

```text
Running application
    ↓
CKB runtime agent / OpenTelemetry source
    ↓
OTLP spans
    ↓
CKB Reality ingestion
    ↓
Exact trace + runtime metrics
    ↓
Source identity resolution
    ↓
Live Execution Twin
    ├── execution feed
    ├── source execution lens
    ├── runtime heat map
    ├── causal neighborhood
    ├── critical-path evidence
    └── architecture-memory investigation
```

The existing Node and Python agents remain the preferred first-party fast path. Standard OTLP-compatible instrumentation can also feed CKB when it supplies enough code identity to resolve an observation safely.

## Identity rule

A runtime identity can be attached directly to a source node only when telemetry supplies a repository-resolvable identity such as a source path plus function.

Example:

```text
src/services/payment.ts::authorizePayment
```

A name such as `authorizePayment` by itself is not enough. Same-named functions can exist in several files. Ambiguous runtime identities remain under CKB's unresolved runtime namespace and are shown as runtime-only evidence until additional identity arrives.

**CKB must never guess a source symbol solely to make a visualization look complete.**

## Exact causal replay

A runtime causal edge is established from an observed parent span and child span in the same trace.

```text
request span
  └─ controller span
      └─ service span
          └─ database span
```

CKB may replay those observed span instances. It must not:

- fill a missing parent with a static caller,
- connect spans from different trace IDs,
- convert a static dependency into a moving runtime edge,
- infer an unobserved request path and present it as executed.

Static topology can remain visible behind a runtime trace, but the evidence classes must stay visually distinct.

## Runtime heat map

The Live Execution Analyzer can classify returned measurements into UI signals such as:

- `hot` — high observed execution count,
- `slow` — observed latency passes the configured threshold,
- `unstable` — observed error rate passes the configured threshold,
- `high-fan-out` — one observed runtime node reaches many distinct observed targets,
- `unresolved-identity` — runtime evidence exists but source identity is not safely established.

These labels are interpretations of supplied measurements, not measurements themselves. Thresholds must be visible/configurable at the product layer when used for operational decisions.

## Dead-code candidate rule

CKB must never claim source is dead merely because no runtime sample happens to exist.

A source node can be labeled only as a **dead-code candidate** when all of the following are true:

1. the source node exists in the static graph,
2. an explicit, valid observation window is known,
3. the node has no runtime metrics in that window,
4. the node does not occur in any exact trace in that window.

Even then the evidence means **not observed during this window**, not **proven unreachable**. Test, scheduled, feature-flagged, disaster-recovery and rare business paths can be valid while remaining unobserved.

## Source execution lens

Function/span telemetry can safely illuminate an exact source-resolved function identity. It does **not** establish individual line execution.

True line-by-line execution requires a separate evidence source such as a profiler, coverage runtime or explicit line instrumentation. Until that exists, the UI must state that limitation and avoid line animation that looks measured.

## Privacy contract

The Live Execution Twin needs structural/runtime metadata, not business payloads.

First-party runtime agents should continue to omit:

- request and response bodies,
- authorization headers and cookies,
- secrets and environment values,
- SQL text and database values,
- Redis keys/values where they may contain sensitive information,
- queue/event payloads,
- WebSocket message contents,
- arbitrary serialized application objects.

Useful safe metadata includes bounded service/source identity, operation type, timing, error state, trace relationships, protocol type and database/messaging system names.

## Tenant and credential boundary

Hosted CKB Cloud must preserve this trust boundary:

```text
Browser user JWT
    ↓
CKB Cloud backend
    ↓ tenant project scoping
server-side Reality credential
    ↓
CKB Reality engine
```

The browser must not receive the internal Reality credential. Any future WebSocket/SSE transport should terminate at the authenticated Cloud boundary or use a short-lived, tenant-scoped stream token issued by that boundary.

## Delivery stages

### Stage 1 — Exact near-real-time twin

- existing Node/Python/OTLP ingestion,
- exact trace persistence,
- authenticated Cloud trace reads,
- rapid refresh/replay in the Live Execution Twin,
- source evidence inspection,
- observed heat map,
- runtime causal neighborhood,
- architecture-memory investigation.

### Stage 2 — Push transport

- tenant-scoped runtime event bus,
- authenticated SSE or WebSocket delivery through CKB Cloud,
- reconnect cursor / last-event identity,
- backpressure and bounded fan-out,
- polling fallback when push is unavailable.

The fallback matters: loss of push transport must degrade freshness, not truth.

### Stage 3 — Broader runtime coverage

Add first-party adapters according to measured demand while retaining native OTLP compatibility:

- JVM/Java,
- Go,
- Rust,
- .NET,
- browser JavaScript,
- Swift/Kotlin mobile,
- containers/Kubernetes,
- infrastructure/eBPF evidence where deployment permissions allow it.

A language or environment is not listed as supported in product UI until an implementation and verification fixture exist.

### Stage 4 — Deeper execution evidence

- explicit line/profiler evidence,
- concurrency/contention views,
- distributed trace correlation across services,
- deployment/version correlation,
- trace-to-test correlation,
- static/runtime drift detection,
- failed-vs-successful trace comparison,
- time-machine replay across architecture snapshots.

## Product rule

> If nothing executed, the runtime universe stays quiet.

That silence is a feature. It proves CKB is showing software reality instead of animation masquerading as intelligence.
