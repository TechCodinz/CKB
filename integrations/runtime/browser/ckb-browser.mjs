const HEX = '0123456789abcdef';

function randomHex(bytes) {
  const values = new Uint8Array(bytes);
  globalThis.crypto.getRandomValues(values);
  let out = '';
  for (const value of values) out += HEX[value >> 4] + HEX[value & 15];
  return out;
}

function nowNano() {
  return String(BigInt(Date.now()) * 1_000_000n);
}

function safeString(value, max = 160) {
  return String(value ?? '').trim().slice(0, max);
}

function safeOrigin(value) {
  try {
    const url = new URL(String(value), globalThis.location?.origin || 'https://local.invalid');
    return url.origin === 'null' ? '' : url.origin;
  } catch {
    return '';
  }
}

const FORBIDDEN_ATTRIBUTE = /(authorization|cookie|token|secret|password|query|body|payload|sql|statement|email|phone|address|user\.id|session)/i;

function safeAttributes(attributes = {}) {
  const out = {};
  for (const [key, raw] of Object.entries(attributes || {})) {
    if (!key || FORBIDDEN_ATTRIBUTE.test(key)) continue;
    if (!['string', 'number', 'boolean'].includes(typeof raw)) continue;
    const value = typeof raw === 'string' ? safeString(raw, 240) : raw;
    out[safeString(key, 100)] = value;
  }
  return out;
}

function traceparent(context) {
  return `00-${context.traceId}-${context.spanId}-01`;
}

function parseTraceparent(value) {
  const match = /^00-([0-9a-f]{32})-([0-9a-f]{16})-([0-9a-f]{2})$/i.exec(String(value || '').trim());
  if (!match || /^0+$/.test(match[1]) || /^0+$/.test(match[2])) return null;
  return { traceId: match[1].toLowerCase(), spanId: match[2].toLowerCase(), flags: match[3].toLowerCase() };
}

function spanStatus(error) {
  return error ? { code: 2, message: 'error observed' } : { code: 1 };
}

/**
 * Browser collector for CKB Live Reality.
 *
 * It deliberately has no built-in CKB credential or direct upload URL. Browser
 * bundles are public artifacts, so callers provide an exporter that forwards
 * batches through their own authenticated backend or another approved broker.
 */
export class CkbBrowserRuntime {
  constructor({ serviceName, exporter, maxBatch = 32, flushMs = 2000 } = {}) {
    if (!serviceName) throw new Error('serviceName is required');
    if (typeof exporter !== 'function') throw new Error('exporter(batch) is required; browser code must not embed a CKB project secret');
    this.serviceName = safeString(serviceName, 120);
    this.exporter = exporter;
    this.maxBatch = Math.max(1, Math.min(Number(maxBatch) || 32, 128));
    this.flushMs = Math.max(250, Math.min(Number(flushMs) || 2000, 30_000));
    this.queue = [];
    this.flushing = null;
    this.timer = null;
  }

  start() {
    if (!this.timer) this.timer = globalThis.setInterval(() => void this.flush(), this.flushMs);
    return this;
  }

  stop() {
    if (this.timer) globalThis.clearInterval(this.timer);
    this.timer = null;
    return this.flush();
  }

  context(parent) {
    return {
      traceId: parent?.traceId || randomHex(16),
      spanId: randomHex(8),
      parentSpanId: parent?.spanId || '',
    };
  }

  begin(name, { parent, route, kind = 'function', attributes = {} } = {}) {
    const context = this.context(parent);
    return {
      context,
      name: safeString(name, 180) || 'browser operation',
      route: route ? safeString(route, 160) : '',
      kind: safeString(kind, 40) || 'function',
      attributes: safeAttributes(attributes),
      startedAt: nowNano(),
    };
  }

  end(span, { error = false, attributes = {} } = {}) {
    const endedAt = nowNano();
    const merged = { ...span.attributes, ...safeAttributes(attributes) };
    const otlpSpan = {
      traceId: span.context.traceId,
      spanId: span.context.spanId,
      ...(span.context.parentSpanId ? { parentSpanId: span.context.parentSpanId } : {}),
      name: span.name,
      startTimeUnixNano: span.startedAt,
      endTimeUnixNano: endedAt,
      status: spanStatus(Boolean(error)),
      attributes: [
        { key: 'service.name', value: { stringValue: this.serviceName } },
        { key: 'ckb.symbol.kind', value: { stringValue: 'browser' } },
        { key: 'ckb.flow.type', value: { stringValue: span.kind } },
        ...(span.route ? [{ key: 'http.route', value: { stringValue: span.route } }] : []),
        ...Object.entries(merged).map(([key, value]) => ({
          key,
          value: typeof value === 'number'
            ? { doubleValue: value }
            : typeof value === 'boolean'
              ? { boolValue: value }
              : { stringValue: value },
        })),
      ],
    };
    this.queue.push(otlpSpan);
    if (this.queue.length >= this.maxBatch) void this.flush();
    return otlpSpan;
  }

  async run(name, options, operation) {
    const span = this.begin(name, options);
    try {
      const value = await operation(span.context);
      this.end(span);
      return value;
    } catch (error) {
      this.end(span, { error: true });
      throw error;
    }
  }

  /**
   * Instrument one fetch without recording raw path/query/header/body data.
   * A developer may supply a stable route template such as `/orders/{id}`.
   */
  async fetch(input, init = {}, { parent, route, fetchImpl = globalThis.fetch } = {}) {
    if (typeof fetchImpl !== 'function') throw new Error('fetch implementation is unavailable');
    const method = safeString(init?.method || 'GET', 20).toUpperCase();
    const origin = safeOrigin(typeof input === 'string' ? input : input?.url);
    const span = this.begin(`${method} ${route || origin || 'request'}`, {
      parent,
      route,
      kind: 'http-client',
      attributes: origin ? { 'server.origin': origin, 'http.request.method': method } : { 'http.request.method': method },
    });
    const headers = new Headers(init?.headers || (typeof input === 'object' ? input?.headers : undefined) || {});
    headers.set('traceparent', traceparent(span.context));
    try {
      const response = await fetchImpl(input, { ...init, headers });
      this.end(span, { error: !response?.ok, attributes: { 'http.response.status_code': Number(response?.status || 0) } });
      return response;
    } catch (error) {
      this.end(span, { error: true });
      throw error;
    }
  }

  async flush() {
    if (this.flushing || !this.queue.length) return this.flushing;
    const spans = this.queue.splice(0, this.maxBatch);
    const payload = {
      resourceSpans: [{
        resource: { attributes: [{ key: 'service.name', value: { stringValue: this.serviceName } }] },
        scopeSpans: [{ scope: { name: 'ckb-browser-runtime', version: '1.0.0' }, spans }],
      }],
    };
    this.flushing = Promise.resolve(this.exporter(payload))
      .catch((error) => {
        this.queue.unshift(...spans);
        this.queue = this.queue.slice(0, this.maxBatch * 4);
        throw error;
      })
      .finally(() => { this.flushing = null; });
    return this.flushing;
  }
}

export { parseTraceparent, safeAttributes, safeOrigin, traceparent };
