# CKB Live Reality — Node.js Runtime Agent

This integration makes the CKB Semantic Universe respond to **observed application execution** rather than decorative graph animation.

The runtime kit has two layers:

- `ckb-live.mjs` — the small evidence emitter and explicit span API.
- `ckb-auto.mjs` — framework/data-flow adapters that automatically create typed runtime transitions for common Node systems.

Both are dependency-free and use OTLP/HTTP JSON. They are designed to make CKB's STATIC / RUNTIME / PREDICTED truth separation stronger, not to manufacture animation.

## What it records

The agent emits structural/runtime metadata needed by CKB:

- `service.name`
- request method + route
- response status
- `code.file.path`
- `code.function.name`
- parent/child trace identity
- start/end timestamps
- error status
- `ckb.flow.type` such as `http-server`, `http-client`, `database`, `cache`, `queue`, `event`, `websocket`, or `function`
- explicitly supplied primitive attributes

It **does not record request bodies, response bodies, SQL text, Redis values, queue payloads, WebSocket messages, cookies, authorization headers, secrets, or arbitrary objects**.

## Environment

Create a project-scoped key from **Living Graph Universe → LIVE LINK** in CKB Cloud, then configure the deployment secrets:

```bash
CKB_OTLP_ENDPOINT=https://ckb-private.onrender.com/api/v1/reality/intelligence/telemetry/otlp
CKB_OTLP_KEY=<project-scoped-live-key>
CKB_SERVICE_NAME=my-api
```

The key can only be used on the runtime ingestion route and only for the project encoded in its permissions. The deployment does not receive the user's CKB browser JWT or the internal Reality Engine secret.

## Fast path — automatic instrumentation

Copy both files into the server project:

```text
ckb-live.mjs
ckb-auto.mjs
```

Then:

```js
import { createCkbAuto } from './ckb-auto.mjs';

export const ckb = createCkbAuto({
  serviceName: 'checkout-api',
});

// Observe outbound global fetch calls. CKB's own telemetry endpoint is excluded
// automatically so the agent never traces its own flush requests.
ckb.installGlobalFetch();
```

### Express

```js
ckb.express(app, {
  file: 'src/server.js',
  namespace: 'checkout-api',
});
```

### NestJS

Nest applications expose `use()`, so the same root request evidence can be attached before `listen()`:

```js
ckb.nest(app, {
  file: 'src/main.ts',
  namespace: 'orders-api',
});
```

### Next.js route handlers

```js
export const POST = ckb.wrapNextHandler(
  'checkout.POST',
  async function POST(request) {
    return handleCheckout(request);
  },
  {
    file: 'app/api/checkout/route.ts',
    functionName: 'POST',
    namespace: 'checkout-route',
  },
);
```

## Automatic data-flow adapters

### Prisma

The Prisma proxy recognizes model CRUD operations and raw-query entrypoints. It records model/operation identity, **not query arguments or SQL**.

```js
const db = ckb.prisma(prisma, {
  file: 'src/db/prisma.ts',
  namespace: 'persistence',
});

await db.order.findMany();
await db.payment.create({ data: payment });
```

### pg / mysql / generic database clients

```js
const db = ckb.instrumentDataClient(pool, {
  system: 'postgresql',
  methods: ['query'],
  file: 'src/db/pool.ts',
});

await db.query('SELECT ...'); // query text is not copied into CKB telemetry
```

### Redis

```js
const cache = ckb.redis(redisClient, {
  file: 'src/cache/redis.ts',
  namespace: 'cache',
});

await cache.get('session-key');
await cache.set('session-key', value);
```

CKB records only the Redis operation name. Keys and values remain outside CKB telemetry.

### Queues / brokers

Wrap producer clients:

```js
const jobs = ckb.producer(queue, {
  system: 'bullmq',
  methods: ['add'],
  file: 'src/queues/jobs.ts',
});

await jobs.add('invoice', payload);
```

Wrap consumer handlers:

```js
const processInvoice = ckb.consumer(
  'invoice.process',
  async job => runInvoice(job),
  {
    system: 'bullmq',
    file: 'src/workers/invoice.ts',
    functionName: 'processInvoice',
  },
);
```

Message bodies are not copied into telemetry.

### Events

```js
emitter.on('order.created', ckb.eventHandler(
  'order.created',
  handleOrderCreated,
  { file: 'src/events/orders.ts', functionName: 'handleOrderCreated' },
));
```

### WebSockets

```js
ckb.websocket(socket, {
  file: 'src/realtime/socket.ts',
  namespace: 'realtime',
});

socket.on('message', ckb.websocketHandler(
  'socket.message',
  handleMessage,
  { file: 'src/realtime/socket.ts', functionName: 'handleMessage' },
));
```

The agent records that a WebSocket send/message transition occurred; it does not record the message body.

## Explicit spans when you want exact source identity

```js
import { createCkbLive } from './ckb-live.mjs';

export const live = createCkbLive({ serviceName: 'checkout-api' });

async function authorizePayment(order) {
  return live.span('authorizePayment', {
    file: 'src/services/payment.js',
    functionName: 'authorizePayment',
    namespace: 'payment',
    flowType: 'function',
  }, async () => paymentGateway.authorize(order));
}
```

Or wrap an existing async function:

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

For synchronous code, use `spanSync` / `wrapSync` so the return type is not changed.

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
  typed flow: HTTP / DB / cache / queue / event / WebSocket
```

That evidence drives directional execution pulses, Live/Fused mode, runtime semantic depth, molecule filtering, fault investigation and exact trace retracing.

## Batching

The default flush interval is 12 seconds to stay efficient on small deployments. CKB deliberately does not encourage one telemetry HTTP request per application span.

```js
const ckb = createCkbAuto({
  serviceName: 'checkout-api',
  flushIntervalMs: 15000,
  maxBatch: 96,
});
```

## Shutdown

```js
process.on('SIGTERM', async () => {
  await ckb.shutdown();
  process.exit(0);
});
```

## Truth contract

A moving edge in CKB must correspond to runtime evidence.

The Node runtime kit emits real observed spans only. Static graph relationships that were not observed remain static. Predictions remain a separate CKB evidence class.
