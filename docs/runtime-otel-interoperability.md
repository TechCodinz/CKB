# CKB Runtime — OpenTelemetry Interoperability Contract

CKB's Live Execution Twin is not limited to first-party agents. Any runtime that can emit OTLP/HTTP JSON can participate **without CKB claiming first-party instrumentation support for that language**.

This document defines the minimum metadata needed for safe source fusion and typed execution flow.

## Minimum span identity

Every observed application span should contain normal OpenTelemetry trace identity:

- `traceId`
- `spanId`
- `parentSpanId` when a real observed parent exists
- `startTimeUnixNano`
- `endTimeUnixNano`
- `status.code`

CKB uses trace/span identity for exact replay. It never fills a missing runtime parent from the static call graph.

## Exact source resolution

For CKB to attach a runtime span directly to a repository source node, emit:

```text
code.file.path       = src/services/payment.ts
code.function.name   = authorizePayment
```

Recommended optional fields:

```text
code.namespace       = payment
ckb.symbol.kind      = function
```

`code.file.name=index.ts` by itself is intentionally insufficient because many repositories contain the same basename. Function or service names without a repository-resolvable path remain runtime-only evidence.

## Typed flow metadata

CKB understands the following language-neutral fields:

```text
ckb.flow.type
ckb.flow.direction
network.protocol.name
db.system
messaging.system
http.request.method
server.address
```

Recommended `ckb.flow.type` values:

- `function`
- `http-server`
- `http-client`
- `database`
- `cache`
- `queue`
- `event`
- `websocket`
- `heartbeat`

Custom values can still be retained as bounded runtime metadata, but the UI should not assign a standard semantic icon/category unless the mapping is known.

## Privacy boundary

Do not export the following solely to power CKB visualization:

- request/response bodies;
- cookies or authorization headers;
- access tokens, secrets or passwords;
- SQL statements or query parameters;
- database values;
- Redis keys/values when they may contain user/session identifiers;
- queue/event payloads;
- WebSocket message content;
- arbitrary serialized application objects.

Prefer operation identity and system identity:

```text
db.system = postgresql
db.operation.name = orders.select
```

instead of:

```text
SELECT * FROM orders WHERE user_email='...'
```

## Deployment configuration

Point an OTLP/HTTP JSON exporter at the project-scoped CKB ingest endpoint supplied by CKB Cloud. The exact authentication header depends on the CKB ingest gateway. First-party CKB agents currently use the project-scoped telemetry key rather than a browser session JWT or internal Reality secret.

The hosted trust boundary remains:

```text
application runtime key → Cloud ingest boundary → tenant scope → Reality engine
browser JWT             → Cloud read boundary   → tenant scope → Reality engine
```

Those credentials are intentionally different.

## Java / JVM

Use OpenTelemetry Java Agent or SDK instrumentation, then add a span processor/instrumentation hook that supplies `code.file.path` where the application can resolve source paths reliably. Standard class/method names are useful runtime identity but should remain unresolved if no repository path can be established.

CKB compatibility does **not** mean CKB has verified every Java framework automatically. Framework-specific first-party support should be listed only after fixtures exist.

## .NET

Use OpenTelemetry .NET tracing and enrich application spans with source path/function identity where reliable. ASP.NET, HttpClient and database instrumentation can supply runtime boundaries; CKB source fusion still follows the exact path rule above.

## Rust

Use `tracing` + OpenTelemetry export or the OpenTelemetry Rust SDK. Instrument important request/function/DB/message boundaries and attach safe `code.file.path`/`code.function.name` attributes when known.

## Go

CKB now includes `integrations/runtime/go`, a first-party standard-library agent with W3C trace propagation, HTTP/function/DB/cache/messaging helpers and bounded OTLP batching. Applications already using OpenTelemetry Go can follow this document instead.

## Browser JavaScript

Browser telemetry should be treated more conservatively because URLs, DOM state and user interaction data can contain personal information. Prefer route templates, component identities and safe timing metadata. Do not export form contents, storage values, cookies, tokens, full DOM snapshots or arbitrary event objects.

Browser spans should correlate with backend traces through standard trace context only where CORS/security policy allows it.

## Swift / Kotlin mobile

Use the platform OpenTelemetry SDK or a small application-specific wrapper to record route/use-case/network boundaries. Do not capture view contents, typed text, authentication headers, device identifiers or request bodies solely for CKB.

## eBPF / infrastructure telemetry

eBPF and host/container telemetry can enrich the Live Execution Twin with process/network evidence, but it is a **different evidence class** from source-resolved application execution.

For example, observing TCP traffic between two processes does not prove which source function made the request. CKB should label it infrastructure-observed until trace/source correlation establishes a stronger identity.

## Product-support rule

CKB Cloud may say **OTLP compatible** when this ingest contract is satisfied.

It may say **first-party agent supported** only when CKB ships and tests that language integration.

As of this feature branch, the repository contains first-party runtime integrations for Node.js, Python and Go. Other languages can integrate through OpenTelemetry using this contract, but must not be advertised as verified first-party adapters until implementation fixtures are added.
