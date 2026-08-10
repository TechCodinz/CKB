# CKB Live Reality — Node.js Runtime Agents

CKB Live Reality makes the Semantic Universe and IDE extensions respond to **observed application execution** rather than decorative graph animation.

The dependency-free Node runtime kit now has three layers:

- `ckb-live.mjs` — tracing core, batching, exact parent/child context and deployment heartbeat.
- `ckb-auto.mjs` — adapters for common server, database, cache, queue, event and WebSocket boundaries.
- `ckb-detect.mjs` — package-metadata-only runtime stack detection and setup suggestions.

## Truth and privacy contract

Application spans are emitted only when code actually executes. The optional heartbeat is explicitly tagged `ckb.runtime.heartbeat=true` and is not an application call/dependency edge.

CKB records structural/runtime metadata such as service name, route/method, status, source file/function identity, parent/child trace ids, timing, error state and typed transmission metadata (`ckb.flow.type`, direction, protocol, DB/messaging system).

It **does not record request/response bodies, cookies, authorization headers, secrets, SQL text, Redis values, queue payloads, WebSocket messages, or arbitrary objects**.

## Environment

Create a project-scoped key from **Living Graph Universe → LIVE LINK** in CKB Cloud, then configure deployment secrets:

```bash
CKB_OTLP_ENDPOINT=https://ckb-private.onrender.com/api/v1/reality/intelligence/telemetry/otlp
CKB_OTLP_KEY=<project-scoped-live-key>
CKB_SERVICE_NAME=my-api
CKB_HEARTBEAT_INTERVAL_MS=60000
```

The key can ingest telemetry only for its project. The deployment never receives the user's browser JWT or CKB's internal Reality Engine secret.

## Fast path — automatic instrumentation

```js
import { createCkbAuto } from './ckb-auto.mjs';

export const ckb = createCkbAuto({ serviceName: 'checkout-api' });
ckb.installGlobalFetch();
ckb.express(app, { file: 'src/server.js', namespace: 'checkout-api' });
```

Supported adapters include Express, NestJS, Next route handlers, global `fetch`, Prisma, generic DB clients, Redis, queue producers/consumers, application events, WebSockets and arbitrary function boundaries. Adapters are explicit and reversible; CKB does not install invasive module-loader hooks.

## Detect the runtime stack first

```js
import { detectRuntimeStack } from './ckb-detect.mjs';

console.log(detectRuntimeStack(process.cwd()));
```

Detection reads `package.json` metadata only. It does not import application modules, execute user code, inspect environment secrets, or inspect traffic. It recognizes common frameworks, databases, caches, messaging systems, WebSocket libraries and HTTP clients and returns suggested CKB adapters.

## Explicit spans for exact source identity

```js
import { createCkbLive } from './ckb-live.mjs';

const live = createCkbLive({ serviceName: 'checkout-api' });

await live.span('authorizePayment', {
  file: 'src/services/payment.js',
  functionName: 'authorizePayment',
  namespace: 'payment',
  flowType: 'function',
}, async () => paymentGateway.authorize(order));
```

Synchronous code can use `spanSync` / `wrapSync`; async code can use `span` / `wrap`.

### Outbound HTTP

```js
await live.fetch(
  'paymentGateway.authorize',
  'https://gateway.example.com/authorize',
  { method: 'POST', body: payload },
  { file: 'src/clients/paymentGateway.js', functionName: 'authorize' },
);
```

The body still goes to the real destination but is **not copied into CKB telemetry**.

## Automatic data-flow examples

### Prisma

```js
const db = ckb.prisma(prisma, {
  file: 'src/db/prisma.ts',
  namespace: 'persistence',
});
await db.order.findMany();
```

CKB records model/operation identity, not query arguments or SQL.

### Redis

```js
const cache = ckb.redis(redisClient, {
  file: 'src/cache/redis.ts',
  namespace: 'cache',
});
await cache.get('session-key');
```

Only the Redis operation is recorded; keys/values stay outside telemetry.

### Queues / events / WebSockets

```js
const jobs = ckb.producer(queue, { system: 'bullmq', methods: ['add'] });
const consume = ckb.consumer('invoice.process', processInvoice, { system: 'bullmq' });
emitter.on('order.created', ckb.eventHandler('order.created', handleOrderCreated));
ckb.websocket(socket);
socket.on('message', ckb.websocketHandler('socket.message', handleMessage));
```

Payloads/messages are not copied into CKB telemetry.

## Deployment heartbeat

A root heartbeat span is emitted every 60 seconds by default when the agent is configured. It proves the deployment-side agent can reach CKB, but because it is parentless and explicitly typed as `heartbeat`, it is not displayed as a fake request/function transmission.

Disable or tune it:

```js
createCkbLive({ heartbeat: false });
createCkbLive({ heartbeatIntervalMs: 120000 });
```

## Why file/function identity matters

An identity such as:

```text
src/services/payment.js::authorizePayment
```

lets CKB fuse static Tree-sitter architecture with actual runtime evidence:

```text
STATIC
  definition
  callers / callees
  dependencies

RUNTIME
  observed invocation count
  latency
  errors
  typed HTTP / DB / cache / queue / event / WebSocket transitions
  exact parent/child trace sequence
```

That drives directional execution pulses, V8 transmission filtering, Live/Fused mode, runtime semantic depth and exact trace retracing in Cloud and supported IDE extensions.

## Batching and shutdown

The default flush interval is 12 seconds; CKB deliberately avoids one telemetry request per span.

```js
const ckb = createCkbAuto({ flushIntervalMs: 15000, maxBatch: 96 });

process.on('SIGTERM', async () => {
  await ckb.shutdown();
  process.exit(0);
});
```

A moving CKB edge must correspond to runtime evidence. Static relationships remain static; predictions remain explicitly PREDICTED.
