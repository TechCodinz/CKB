# CKB Live Reality — Node.js Runtime Agent

This integration makes the CKB Semantic Universe respond to **observed application execution** rather than decorative graph animation.

It is intentionally small and dependency-free so a deployed Node.js service can begin sending evidence before the full native OpenTelemetry adapter family is installed.

## What it records

The agent emits OTLP/HTTP JSON spans containing only structural/runtime metadata needed by CKB:

- `service.name`
- request method + route (Express middleware)
- response status
- `code.file.path`
- `code.function.name`
- parent/child trace identity
- start/end timestamps
- error status
- explicitly supplied primitive attributes

It **does not record request bodies, response bodies, cookies, authorization headers, secrets, query payloads, or arbitrary objects**.

## Environment

Create a project-scoped key from **Living Graph Universe → LIVE LINK** in CKB Cloud, then configure the deployment secrets:

```bash
CKB_OTLP_ENDPOINT=https://ckb-private.onrender.com/api/v1/reality/intelligence/telemetry/otlp
CKB_OTLP_KEY=<project-scoped-live-key>
CKB_SERVICE_NAME=my-api
```

The key can only be used on the runtime ingestion route and only for the project encoded in its permissions. The deployment does not receive the user's CKB browser JWT or the internal Reality Engine secret.

## Basic usage

Copy `ckb-live.mjs` into the server project or vendor it through your preferred dependency workflow.

```js
import { createCkbLive } from './ckb-live.mjs';

export const live = createCkbLive({
  serviceName: 'checkout-api',
});
```

### Express request entrypoints

```js
app.use(live.expressMiddleware({
  file: 'src/server.js',
  namespace: 'checkout-api',
}));
```

Each HTTP request becomes a root runtime span. Child CKB spans created during that request automatically inherit the trace identity via `AsyncLocalStorage`.

### Observe internal functions

```js
async function authorizePayment(order) {
  return live.span('authorizePayment', {
    file: 'src/services/payment.js',
    functionName: 'authorizePayment',
    namespace: 'payment',
  }, async () => {
    return paymentGateway.authorize(order);
  });
}
```

Or wrap an existing function:

```js
const observedAuthorize = live.wrap(
  'authorizePayment',
  authorizePayment,
  {
    file: 'src/services/payment.js',
    functionName: 'authorizePayment',
    namespace: 'payment',
  },
);
```

### Observe outbound HTTP transitions

```js
const response = await live.fetch(
  'paymentGateway.authorize',
  'https://gateway.example.com/authorize',
  { method: 'POST', body: payload },
  {
    file: 'src/clients/paymentGateway.js',
    functionName: 'authorize',
    namespace: 'payment-gateway-client',
  },
);
```

The request body is **not** copied into CKB telemetry.

## Why file/function metadata matters

CKB already knows the static repository through Tree-sitter. Runtime spans become most powerful when their identity can be resolved back to the same static node.

For example:

```text
src/services/payment.js::authorizePayment
```

lets CKB fuse:

```text
STATIC
  function definition
  callers / callees
  dependencies

RUNTIME
  observed invocation count
  latency
  errors
  exact parent/child trace sequence
```

That is what drives V7 features such as directional execution pulses, Live/Fused mode, runtime semantic depth and exact trace retracing.

## Batching

The default flush interval is 12 seconds to stay efficient on small deployments and within the current cloud request boundary. You can tune it, but CKB deliberately does not encourage one telemetry HTTP request per application span.

```js
const live = createCkbLive({
  serviceName: 'checkout-api',
  flushIntervalMs: 15000,
  maxBatch: 96,
});
```

## Shutdown

```js
process.on('SIGTERM', async () => {
  await live.shutdown();
  process.exit(0);
});
```

## Truth contract

A moving edge in CKB must correspond to runtime evidence.

The Node agent therefore emits real observed spans only. Static graph relationships that were not observed remain static. Predictions remain a separate CKB evidence class.
