import { createCkbLive } from './ckb-live.mjs';

const DEFAULT_PRISMA_METHODS = new Set([
  'findUnique','findUniqueOrThrow','findFirst','findFirstOrThrow','findMany','create','createMany','createManyAndReturn',
  'update','updateMany','upsert','delete','deleteMany','count','aggregate','groupBy',
  '$queryRaw','$queryRawUnsafe','$executeRaw','$executeRawUnsafe',
]);

const DEFAULT_REDIS_METHODS = new Set([
  'get','getBuffer','set','setex','setEx','mget','mset','del','unlink','exists','expire','ttl','incr','incrby','decr','decrby',
  'hget','hset','hmget','hmset','hdel','hgetall','lpush','rpush','lpop','rpop','sadd','srem','smembers','zadd','zrem','zrange',
  'xadd','xread','xreadgroup','publish','sendCommand',
]);

function hostnameOf(input) {
  try { return new URL(String(input)).hostname; } catch { return ''; }
}

function metadata(base = {}, patch = {}) {
  return {
    ...base,
    ...patch,
    attributes: { ...(base.attributes || {}), ...(patch.attributes || {}) },
  };
}

function methodProxy(target, onMethod, shouldWrap) {
  const cache = new Map();
  return new Proxy(target, {
    get(object, prop, receiver) {
      const value = Reflect.get(object, prop, receiver);
      if (typeof value !== 'function' || !shouldWrap(String(prop), object)) return value;
      if (cache.has(prop)) return cache.get(prop);
      const wrapped = function ckbInstrumentedMethod(...args) {
        return onMethod(String(prop), value, object, args, this);
      };
      cache.set(prop, wrapped);
      return wrapped;
    },
  });
}

function recursiveMethodProxy(target, options, path = [], seen = new WeakMap()) {
  if (!target || (typeof target !== 'object' && typeof target !== 'function')) return target;
  if (seen.has(target)) return seen.get(target);

  const proxy = new Proxy(target, {
    get(object, prop, receiver) {
      const value = Reflect.get(object, prop, receiver);
      const name = String(prop);
      const nextPath = [...path, name];
      if (typeof value === 'function' && options.shouldWrap(name, nextPath, object)) {
        return function ckbRecursiveObserved(...args) {
          return options.onMethod(name, nextPath, value, object, args, this);
        };
      }
      if (value && (typeof value === 'object' || typeof value === 'function')) {
        return recursiveMethodProxy(value, options, nextPath, seen);
      }
      return value;
    },
  });
  seen.set(target, proxy);
  return proxy;
}

/**
 * High-level Node.js instrumentation for CKB Live Reality.
 *
 * It favors explicit, reversible adapters over invasive module-loader hooks.
 * No adapter records payloads, SQL text, Redis values, queue bodies, WebSocket
 * messages, HTTP bodies, cookies, authorization headers, or arbitrary objects.
 */
export function createCkbAuto(options = {}) {
  const live = options.live || createCkbLive(options);
  const disposers = new Set();
  const endpointHost = hostnameOf(options.endpoint || process.env.CKB_OTLP_ENDPOINT || '');

  function dispose(disposer) {
    if (typeof disposer === 'function') disposers.add(disposer);
    return disposer;
  }

  function installGlobalFetch(config = {}) {
    if (typeof globalThis.fetch !== 'function') return () => {};
    const original = globalThis.fetch;
    if (original.__ckbInstrumentedFetch) return () => {};
    const ignoredHosts = new Set([endpointHost, ...(config.ignoreHosts || [])].filter(Boolean));

    async function ckbObservedFetch(input, init = {}) {
      const host = hostnameOf(input?.url || input);
      if (ignoredHosts.has(host)) return original.call(this, input, init);
      const method = String(init?.method || input?.method || 'GET').toUpperCase();
      return live.span(`HTTP ${method} ${host || 'external'}`, metadata(config.metadata, {
        kind: 'outbound-http',
        flowType: 'http-client',
        direction: 'outbound',
        functionName: config.functionName || 'global.fetch',
        attributes: {
          'http.request.method': method,
          'server.address': host,
          'network.protocol.name': 'http',
        },
      }), () => original.call(this, input, init));
    }

    Object.defineProperty(ckbObservedFetch, '__ckbInstrumentedFetch', { value: true });
    Object.defineProperty(ckbObservedFetch, '__ckbOriginalFetch', { value: original });
    globalThis.fetch = ckbObservedFetch;
    return dispose(() => {
      if (globalThis.fetch === ckbObservedFetch) globalThis.fetch = original;
    });
  }

  function express(app, config = {}) {
    if (!app || typeof app.use !== 'function') throw new TypeError('CKB Express instrumentation requires app.use().');
    const middleware = live.expressMiddleware({
      file: config.file,
      namespace: config.namespace || config.serviceName,
      functionName: config.functionName || 'express.request',
    });
    app.use(middleware);
    return middleware;
  }

  function nest(app, config = {}) {
    if (!app || typeof app.use !== 'function') throw new TypeError('CKB Nest instrumentation requires an application exposing use().');
    return express(app, {
      ...config,
      namespace: config.namespace || 'nestjs',
      functionName: config.functionName || 'nest.request',
    });
  }

  function wrapNextHandler(name, handler, config = {}) {
    if (typeof handler !== 'function') throw new TypeError('CKB Next handler instrumentation requires a function.');
    return async function ckbNextHandler(request, context) {
      const url = request?.url || request?.nextUrl?.pathname || '';
      const method = String(request?.method || 'HTTP').toUpperCase();
      return live.span(name || `NEXT ${method}`, metadata(config, {
        kind: 'route',
        flowType: 'http-server',
        direction: 'inbound',
        functionName: config.functionName || name || 'next.handler',
        attributes: {
          'http.request.method': method,
          'http.route': (() => { try { return new URL(String(url)).pathname; } catch { return String(url).slice(0, 220); } })(),
          'network.protocol.name': 'http',
        },
      }), () => handler(request, context));
    };
  }

  function instrumentDataClient(client, config = {}) {
    if (!client) throw new TypeError('CKB data-client instrumentation requires a client instance.');
    const methods = new Set((config.methods || ['query','execute','run','all','get']).map(String));
    const system = String(config.system || 'database');
    return methodProxy(
      client,
      (method, fn, owner, args) => live.span(`${system}.${method}`, metadata(config, {
        kind: 'database',
        flowType: 'database',
        direction: 'outbound',
        functionName: `${system}.${method}`,
        attributes: {
          'db.system': system,
          'db.operation.name': method,
          'ckb.data.capture': 'metadata-only',
        },
      }), () => fn.apply(owner, args)),
      method => methods.has(method),
    );
  }

  function prisma(client, config = {}) {
    if (!client) throw new TypeError('CKB Prisma instrumentation requires a Prisma client.');
    return recursiveMethodProxy(client, {
      shouldWrap(method, path) {
        if (!DEFAULT_PRISMA_METHODS.has(method)) return false;
        if (path.some(part => part === 'then' || part === 'catch' || part === 'finally')) return false;
        return true;
      },
      onMethod(method, path, fn, owner, args) {
        const model = path.length > 1 ? path[path.length - 2] : 'prisma';
        return live.span(`prisma.${model}.${method}`, metadata(config, {
          kind: 'database',
          flowType: 'database',
          direction: 'outbound',
          functionName: config.functionName || `prisma.${model}.${method}`,
          attributes: {
            'db.system': config.system || 'prisma',
            'db.operation.name': method,
            'db.namespace': model,
            'ckb.data.capture': 'metadata-only',
          },
        }), () => fn.apply(owner, args));
      },
    });
  }

  function redis(client, config = {}) {
    if (!client) throw new TypeError('CKB Redis instrumentation requires a Redis client.');
    const methods = new Set((config.methods || Array.from(DEFAULT_REDIS_METHODS)).map(String));
    return methodProxy(
      client,
      (method, fn, owner, args) => live.span(`redis.${method}`, metadata(config, {
        kind: 'cache',
        flowType: 'cache',
        direction: method === 'subscribe' ? 'inbound' : 'outbound',
        functionName: config.functionName || `redis.${method}`,
        attributes: {
          'db.system': 'redis',
          'db.operation.name': method,
          'ckb.data.capture': 'metadata-only',
        },
      }), () => fn.apply(owner, args)),
      method => methods.has(method),
    );
  }

  function producer(client, config = {}) {
    if (!client) throw new TypeError('CKB queue producer instrumentation requires a client.');
    const methods = new Set((config.methods || ['add','send','publish','produce','emit']).map(String));
    const system = String(config.system || 'queue');
    return methodProxy(
      client,
      (method, fn, owner, args) => live.span(`${system}.${method}`, metadata(config, {
        kind: 'queue-producer',
        flowType: 'queue',
        direction: 'outbound',
        functionName: config.functionName || `${system}.${method}`,
        attributes: {
          'messaging.system': system,
          'messaging.operation.name': method,
          'ckb.data.capture': 'metadata-only',
        },
      }), () => fn.apply(owner, args)),
      method => methods.has(method),
    );
  }

  function consumer(name, handler, config = {}) {
    if (typeof handler !== 'function') throw new TypeError('CKB queue consumer instrumentation requires a handler function.');
    const system = String(config.system || 'queue');
    return function ckbQueueConsumer(...args) {
      return live.span(name || `${system}.consume`, metadata(config, {
        kind: 'queue-consumer',
        flowType: 'queue',
        direction: 'inbound',
        functionName: config.functionName || name || `${system}.consume`,
        attributes: {
          'messaging.system': system,
          'messaging.operation.name': 'process',
          'ckb.data.capture': 'metadata-only',
        },
      }), () => handler.apply(this, args));
    };
  }

  function eventHandler(name, handler, config = {}) {
    if (typeof handler !== 'function') throw new TypeError('CKB event instrumentation requires a handler function.');
    return function ckbEventHandler(...args) {
      return live.span(name || 'event.handle', metadata(config, {
        kind: 'event-handler',
        flowType: 'event',
        direction: config.direction || 'inbound',
        functionName: config.functionName || name || 'event.handle',
        attributes: {
          'event.name': String(name || 'event').slice(0, 160),
          'ckb.data.capture': 'metadata-only',
        },
      }), () => handler.apply(this, args));
    };
  }

  function websocket(socket, config = {}) {
    if (!socket) throw new TypeError('CKB WebSocket instrumentation requires a socket.');
    const originalSend = typeof socket.send === 'function' ? socket.send : null;
    if (originalSend && !originalSend.__ckbObserved) {
      const observedSend = function ckbWebSocketSend(...args) {
        return live.span('websocket.send', metadata(config, {
          kind: 'websocket', flowType: 'websocket', direction: 'outbound',
          functionName: config.functionName || 'websocket.send',
          attributes: { 'network.protocol.name': 'websocket', 'ckb.data.capture': 'metadata-only' },
        }), () => originalSend.apply(this, args));
      };
      Object.defineProperty(observedSend, '__ckbObserved', { value: true });
      socket.send = observedSend;
      dispose(() => { if (socket.send === observedSend) socket.send = originalSend; });
    }
    return socket;
  }

  function websocketHandler(name, handler, config = {}) {
    if (typeof handler !== 'function') throw new TypeError('CKB WebSocket handler instrumentation requires a function.');
    return function ckbWebSocketHandler(...args) {
      return live.span(name || 'websocket.message', metadata(config, {
        kind: 'websocket', flowType: 'websocket', direction: 'inbound',
        functionName: config.functionName || name || 'websocket.message',
        attributes: { 'network.protocol.name': 'websocket', 'ckb.data.capture': 'metadata-only' },
      }), () => handler.apply(this, args));
    };
  }

  function functionBoundary(name, fn, config = {}) {
    return live.wrap(name, fn, metadata(config, {
      kind: config.kind || 'function',
      flowType: config.flowType || 'function',
      direction: config.direction || 'internal',
    }));
  }

  async function shutdown() {
    for (const disposer of Array.from(disposers).reverse()) {
      try { disposer(); } catch { /* best-effort restore */ }
    }
    disposers.clear();
    return live.shutdown();
  }

  return {
    live,
    installGlobalFetch,
    express,
    nest,
    wrapNextHandler,
    instrumentDataClient,
    prisma,
    redis,
    producer,
    consumer,
    eventHandler,
    websocket,
    websocketHandler,
    functionBoundary,
    shutdown,
  };
}

export default createCkbAuto;
