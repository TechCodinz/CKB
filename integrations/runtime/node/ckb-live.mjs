import { AsyncLocalStorage } from 'node:async_hooks';
import { randomBytes } from 'node:crypto';

const contextStore = new AsyncLocalStorage();
const hex = bytes => randomBytes(bytes).toString('hex');
const nanoNow = () => (BigInt(Date.now()) * 1_000_000n).toString();

function otlpValue(value) {
  if (typeof value === 'boolean') return { boolValue: value };
  if (typeof value === 'number' && Number.isFinite(value)) return { doubleValue: value };
  return { stringValue: String(value ?? '') };
}

function safeAttributes(input = {}) {
  const output = [];
  for (const [key, raw] of Object.entries(input || {})) {
    if (raw === undefined || raw === null) continue;
    if (typeof raw === 'object') continue;
    output.push({ key: String(key).slice(0, 120), value: otlpValue(raw) });
  }
  return output.slice(0, 64);
}

function flowAttributes(metadata = {}, fallback = 'function') {
  return {
    'ckb.flow.type': metadata.flowType || metadata?.attributes?.['ckb.flow.type'] || fallback,
    'ckb.flow.direction': metadata.direction || metadata?.attributes?.['ckb.flow.direction'],
  };
}

/**
 * CKB Live Reality agent for Node.js 18+.
 *
 * - zero runtime dependencies
 * - batches OTLP/HTTP JSON every ~12s by default
 * - preserves parent/child trace identity with AsyncLocalStorage
 * - never records request/response bodies, secrets, headers or arbitrary objects
 * - emits code.file.path + code.function.name so CKB can map observed execution
 *   back onto the static Tree-sitter architecture when those identities match
 * - emits ckb.flow.type so the Semantic Universe can distinguish HTTP, database,
 *   cache, queue, event, WebSocket and internal function transitions.
 */
export function createCkbLive(options = {}) {
  const endpoint = String(options.endpoint || process.env.CKB_OTLP_ENDPOINT || '').trim();
  const key = String(options.key || process.env.CKB_OTLP_KEY || '').trim();
  const serviceName = String(options.serviceName || process.env.CKB_SERVICE_NAME || process.env.RENDER_SERVICE_NAME || process.env.VERCEL_PROJECT_PRODUCTION_URL || 'node-service');
  const environment = String(options.environment || process.env.NODE_ENV || 'unknown');
  const flushIntervalMs = Math.max(10_000, Number(options.flushIntervalMs || 12_000));
  const maxBatch = Math.max(8, Math.min(256, Number(options.maxBatch || 96)));
  const queue = [];
  let timer = null;
  let flushing = false;
  let stopped = false;

  function configured() {
    return Boolean(endpoint && key);
  }

  function currentContext() {
    const value = contextStore.getStore();
    return value ? { traceId: value.traceId, spanId: value.spanId } : null;
  }

  function resourceAttributes() {
    return safeAttributes({
      'service.name': serviceName,
      'deployment.environment': environment,
      'telemetry.sdk.name': 'ckb-live-reality',
      'telemetry.sdk.language': 'nodejs',
      'ckb.runtime.agent': 'node-zero-dependency-v2',
    });
  }

  function schedule() {
    if (timer || stopped || !configured()) return;
    timer = setTimeout(() => {
      timer = null;
      void flush();
    }, flushIntervalMs);
    timer.unref?.();
  }

  function enqueue(span) {
    if (!configured() || stopped) return;
    queue.push(span);
    if (queue.length >= maxBatch) void flush();
    else schedule();
  }

  async function flush() {
    if (!configured() || stopped || flushing || queue.length === 0) return { sent: 0 };
    flushing = true;
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    const batch = queue.splice(0, maxBatch);
    try {
      const response = await fetch(endpoint, {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          'x-ckb-telemetry-key': key,
          'user-agent': 'CKB-Live-Reality-Node/2.0',
        },
        body: JSON.stringify({
          resourceSpans: [{
            resource: { attributes: resourceAttributes() },
            scopeSpans: [{
              scope: { name: 'ckb.live.reality', version: '2.0.0' },
              spans: batch,
            }],
          }],
        }),
      });
      if (!response.ok) {
        queue.unshift(...batch.slice(-maxBatch));
        return { sent: 0, status: response.status };
      }
      return { sent: batch.length, status: response.status };
    } catch (error) {
      queue.unshift(...batch.slice(-maxBatch));
      return { sent: 0, error: error instanceof Error ? error.message : String(error) };
    } finally {
      flushing = false;
      if (queue.length) schedule();
    }
  }

  function makeSpanRecord(name, metadata, context, startTimeUnixNano, error) {
    const attrs = {
      'code.function.name': metadata?.functionName || name,
      'code.file.path': metadata?.file || metadata?.path,
      'code.namespace': metadata?.namespace,
      'ckb.symbol.kind': metadata?.kind || 'function',
      'ckb.runtime.observed': true,
      ...flowAttributes(metadata, metadata?.kind || 'function'),
      ...metadata?.attributes,
    };
    return {
      traceId: context.traceId,
      spanId: context.spanId,
      parentSpanId: context.parentSpanId,
      name: String(name || metadata?.functionName || 'function'),
      startTimeUnixNano,
      endTimeUnixNano: nanoNow(),
      attributes: safeAttributes(attrs),
      status: { code: error ? 2 : 1 },
    };
  }

  async function span(name, metadata, fn) {
    if (typeof metadata === 'function') {
      fn = metadata;
      metadata = {};
    }
    if (typeof fn !== 'function') throw new TypeError('CKB span requires a function to execute.');

    const parent = contextStore.getStore();
    const context = {
      traceId: parent?.traceId || hex(16),
      spanId: hex(8),
      parentSpanId: parent?.spanId || '',
    };
    const startTimeUnixNano = nanoNow();
    let error = null;

    try {
      return await contextStore.run({ traceId: context.traceId, spanId: context.spanId }, () => Promise.resolve(fn()));
    } catch (caught) {
      error = caught;
      throw caught;
    } finally {
      enqueue(makeSpanRecord(name, metadata || {}, context, startTimeUnixNano, error));
    }
  }

  function spanSync(name, metadata, fn) {
    if (typeof metadata === 'function') {
      fn = metadata;
      metadata = {};
    }
    if (typeof fn !== 'function') throw new TypeError('CKB spanSync requires a function to execute.');

    const parent = contextStore.getStore();
    const context = {
      traceId: parent?.traceId || hex(16),
      spanId: hex(8),
      parentSpanId: parent?.spanId || '',
    };
    const startTimeUnixNano = nanoNow();
    let error = null;

    try {
      return contextStore.run({ traceId: context.traceId, spanId: context.spanId }, fn);
    } catch (caught) {
      error = caught;
      throw caught;
    } finally {
      enqueue(makeSpanRecord(name, metadata || {}, context, startTimeUnixNano, error));
    }
  }

  function wrap(name, fn, metadata = {}) {
    if (typeof fn !== 'function') throw new TypeError('CKB wrap requires a function.');
    return function ckbObservedFunction(...args) {
      return span(name, metadata, () => fn.apply(this, args));
    };
  }

  function wrapSync(name, fn, metadata = {}) {
    if (typeof fn !== 'function') throw new TypeError('CKB wrapSync requires a function.');
    return function ckbObservedSyncFunction(...args) {
      return spanSync(name, metadata, () => fn.apply(this, args));
    };
  }

  function expressMiddleware(options = {}) {
    return function ckbExpressReality(req, res, next) {
      if (!configured()) return next();
      const traceId = hex(16);
      const spanId = hex(8);
      const startTimeUnixNano = nanoNow();
      let finished = false;

      const finish = error => {
        if (finished) return;
        finished = true;
        const route = req.route?.path || req.path || req.url || '/';
        enqueue({
          traceId,
          spanId,
          parentSpanId: '',
          name: `${req.method || 'HTTP'} ${route}`,
          startTimeUnixNano,
          endTimeUnixNano: nanoNow(),
          attributes: safeAttributes({
            'http.request.method': req.method || '',
            'http.route': String(route).slice(0, 220),
            'http.response.status_code': Number(res.statusCode || 0),
            'network.protocol.name': 'http',
            'code.function.name': options.functionName || 'express.request',
            'code.file.path': options.file,
            'code.namespace': options.namespace || serviceName,
            'ckb.symbol.kind': 'route',
            'ckb.runtime.observed': true,
            'ckb.flow.type': 'http-server',
            'ckb.flow.direction': 'inbound',
          }),
          status: { code: error || Number(res.statusCode || 0) >= 500 ? 2 : 1 },
        });
      };

      res.once('finish', () => finish(null));
      res.once('close', () => finish(res.writableEnded ? null : new Error('connection closed')));
      contextStore.run({ traceId, spanId }, () => {
        try { next(); } catch (error) { finish(error); throw error; }
      });
    };
  }

  function fetchObserved(name, url, init = {}, metadata = {}) {
    return span(name || `fetch ${String(url)}`, {
      ...metadata,
      kind: metadata.kind || 'outbound-http',
      flowType: metadata.flowType || 'http-client',
      direction: metadata.direction || 'outbound',
      attributes: {
        'server.address': (() => { try { return new URL(String(url)).hostname; } catch { return ''; } })(),
        'http.request.method': String(init?.method || 'GET').toUpperCase(),
        'network.protocol.name': 'http',
        ...metadata.attributes,
      },
    }, () => fetch(url, init));
  }

  async function shutdown() {
    if (timer) clearTimeout(timer);
    timer = null;
    const result = await flush();
    stopped = true;
    return result;
  }

  schedule();
  return {
    configured,
    currentContext,
    span,
    spanSync,
    wrap,
    wrapSync,
    expressMiddleware,
    fetch: fetchObserved,
    flush,
    shutdown,
    queueDepth: () => queue.length,
  };
}

export default createCkbLive;
