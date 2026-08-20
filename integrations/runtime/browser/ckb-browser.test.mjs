import assert from 'node:assert/strict';
import test from 'node:test';
import { CkbBrowserRuntime, parseTraceparent, safeAttributes, safeOrigin, traceparent } from './ckb-browser.mjs';

test('traceparent is valid and round-trips', () => {
  const runtime = new CkbBrowserRuntime({ serviceName: 'storefront', exporter: async () => {} });
  const span = runtime.begin('click checkout');
  const header = traceparent(span.context);
  const parsed = parseTraceparent(header);
  assert.equal(parsed.traceId, span.context.traceId);
  assert.equal(parsed.spanId, span.context.spanId);
});

test('raw URL path and query never become automatic browser telemetry', async () => {
  const batches = [];
  const runtime = new CkbBrowserRuntime({ serviceName: 'storefront', exporter: async payload => batches.push(payload), maxBatch: 10 });
  let propagated = '';
  const response = await runtime.fetch('https://shop.example/orders/customer-123?token=private', {}, {
    route: '/orders/{id}',
    fetchImpl: async (_input, init) => {
      propagated = init.headers.get('traceparent');
      return { ok: true, status: 200 };
    },
  });
  assert.equal(response.status, 200);
  assert.ok(parseTraceparent(propagated));
  await runtime.flush();
  const serialized = JSON.stringify(batches);
  assert.match(serialized, /\/orders\/\{id\}/);
  assert.match(serialized, /https:\/\/shop\.example/);
  assert.doesNotMatch(serialized, /customer-123/);
  assert.doesNotMatch(serialized, /token=private/);
});

test('sensitive custom attributes are filtered', () => {
  const filtered = safeAttributes({
    feature: 'checkout',
    authorization: 'Bearer private',
    sessionToken: 'private',
    requestBody: '{secret}',
    count: 3,
  });
  assert.deepEqual(filtered, { feature: 'checkout', count: 3 });
});

test('origin sanitizer drops path and query', () => {
  assert.equal(safeOrigin('https://example.com/private/abc?email=user@example.com'), 'https://example.com');
});

test('collector requires application-controlled exporter instead of embedded secret', () => {
  assert.throws(() => new CkbBrowserRuntime({ serviceName: 'storefront' }), /exporter/);
});
