import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const GROUPS = {
  frameworks: {
    express: ['express'],
    nest: ['@nestjs/core', '@nestjs/common'],
    next: ['next'],
    fastify: ['fastify'],
    koa: ['koa'],
    hapi: ['@hapi/hapi'],
  },
  databases: {
    prisma: ['@prisma/client', 'prisma'],
    postgres: ['pg', 'postgres'],
    mysql: ['mysql', 'mysql2'],
    sqlite: ['sqlite3', 'better-sqlite3'],
    mongodb: ['mongodb', 'mongoose'],
    sequelize: ['sequelize'],
    typeorm: ['typeorm'],
    drizzle: ['drizzle-orm'],
  },
  caches: {
    redis: ['redis', 'ioredis'],
    memcached: ['memcached'],
  },
  messaging: {
    bullmq: ['bullmq'],
    bull: ['bull'],
    kafka: ['kafkajs', 'node-rdkafka'],
    rabbitmq: ['amqplib'],
    sqs: ['@aws-sdk/client-sqs'],
    pubsub: ['@google-cloud/pubsub'],
  },
  websockets: {
    ws: ['ws'],
    socketio: ['socket.io', 'socket.io-client'],
  },
  httpClients: {
    axios: ['axios'],
    undici: ['undici'],
    got: ['got'],
    nodefetch: ['node-fetch'],
  },
};

function loadPackage(rootDir) {
  const file = resolve(rootDir || process.cwd(), 'package.json');
  if (!existsSync(file)) return { file, package: null, dependencies: {} };
  try {
    const parsed = JSON.parse(readFileSync(file, 'utf8'));
    return {
      file,
      package: parsed,
      dependencies: {
        ...(parsed.dependencies || {}),
        ...(parsed.devDependencies || {}),
        ...(parsed.optionalDependencies || {}),
        ...(parsed.peerDependencies || {}),
      },
    };
  } catch (error) {
    return { file, package: null, dependencies: {}, error: error instanceof Error ? error.message : String(error) };
  }
}

function detectGroup(dependencies, group) {
  return Object.entries(group)
    .filter(([, packages]) => packages.some(name => Object.prototype.hasOwnProperty.call(dependencies, name)))
    .map(([name]) => name);
}

/**
 * Inspects package metadata only. It never loads application modules, executes
 * user code, reads environment secrets or inspects request/data payloads.
 */
export function detectRuntimeStack(rootDir = process.cwd()) {
  const loaded = loadPackage(rootDir);
  const dependencies = loaded.dependencies || {};
  const result = {
    rootDir: resolve(rootDir || process.cwd()),
    packageFile: loaded.file,
    packageName: loaded.package?.name || '',
    packageVersion: loaded.package?.version || '',
    frameworks: detectGroup(dependencies, GROUPS.frameworks),
    databases: detectGroup(dependencies, GROUPS.databases),
    caches: detectGroup(dependencies, GROUPS.caches),
    messaging: detectGroup(dependencies, GROUPS.messaging),
    websockets: detectGroup(dependencies, GROUPS.websockets),
    httpClients: detectGroup(dependencies, GROUPS.httpClients),
    detectedAt: new Date().toISOString(),
    source: 'package-metadata-only',
    error: loaded.error,
  };

  const suggestions = [];
  if (result.frameworks.includes('express')) suggestions.push('auto.express(app)');
  if (result.frameworks.includes('nest')) suggestions.push('auto.nest(app)');
  if (result.frameworks.includes('next')) suggestions.push('auto.wrapNextHandler(name, handler)');
  if (result.databases.includes('prisma')) suggestions.push('prisma = auto.prisma(prisma)');
  if (result.caches.includes('redis')) suggestions.push('redis = auto.redis(redis)');
  if (result.messaging.length) suggestions.push('wrap producers with auto.producer(...) and consumers with auto.consumer(...)');
  if (result.websockets.length) suggestions.push('auto.websocket(socket) / auto.websocketHandler(...)');
  suggestions.push('auto.installGlobalFetch() for outbound HTTP transitions');

  return { ...result, suggestions };
}

export function describeRuntimeStack(rootDir = process.cwd()) {
  const stack = detectRuntimeStack(rootDir);
  const categories = ['frameworks', 'databases', 'caches', 'messaging', 'websockets', 'httpClients'];
  const lines = categories
    .map(key => `${key}: ${(stack[key] || []).join(', ') || 'none detected'}`)
    .join('\n');
  return `CKB runtime stack detection (${stack.source})\n${lines}`;
}

export default detectRuntimeStack;
