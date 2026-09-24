#!/usr/bin/env node
import { createHash, createHmac, timingSafeEqual } from 'node:crypto'
import { createServer } from 'node:http'
import { spawn } from 'node:child_process'
import { mkdir, readFile, rename, rm, writeFile, copyFile } from 'node:fs/promises'
import path from 'node:path'

const listenHost = process.env.OMNICODE_RELEASE_HOST || '127.0.0.1'
const listenPort = Number(process.env.OMNICODE_RELEASE_PORT || 18085)
const publicIp = process.env.OMNICODE_VPS_IP || '169.58.175.192'
const rootDir = process.env.OMNICODE_RELEASE_ROOT || '/opt/app-platform/generated/omnicode'
const caddyFile = process.env.OMNICODE_CADDYFILE || '/etc/caddy/Caddyfile'
const rootSecret = (
  process.env.OMNICODE_CKB_SHARED_SECRET ||
  process.env.OMNICODE_CKB_FEEDBACK_SECRET ||
  ''
).trim()

if (!rootSecret) {
  console.error('[omnicode-release-agent] missing shared trust root')
  process.exit(1)
}

const releaseSecret = createHmac('sha256', rootSecret)
  .update('omnicode-vps-release-v1')
  .digest('hex')

const stateFile = path.join(rootDir, 'release-state.json')
let deploymentChain = Promise.resolve()

function safeEqual(a, b) {
  const left = Buffer.from(String(a || ''))
  const right = Buffer.from(String(b || ''))
  return left.length === right.length && timingSafeEqual(left, right)
}

function json(res, status, body) {
  const payload = Buffer.from(JSON.stringify(body))
  res.writeHead(status, {
    'content-type': 'application/json; charset=utf-8',
    'content-length': String(payload.length),
    'cache-control': 'no-store',
  })
  res.end(payload)
}

async function bodyJson(req, limit = 8 * 1024 * 1024) {
  const chunks = []
  let bytes = 0
  for await (const chunk of req) {
    bytes += chunk.length
    if (bytes > limit) throw new Error('request_too_large')
    chunks.push(chunk)
  }
  if (!chunks.length) return {}
  return JSON.parse(Buffer.concat(chunks).toString('utf8'))
}

function authorized(req) {
  return safeEqual(req.headers['x-omnicode-release-secret'], releaseSecret)
}

function safeArtifactPath(root, relative) {
  if (
    typeof relative !== 'string' ||
    !relative ||
    relative.length > 240 ||
    relative.startsWith('/') ||
    relative.includes('..') ||
    relative.includes('\\')
  ) throw new Error(`unsafe_artifact_path:${relative}`)

  const resolved = path.resolve(root, relative)
  const base = path.resolve(root) + path.sep
  if (!resolved.startsWith(base)) throw new Error(`unsafe_artifact_path:${relative}`)
  return resolved
}

function artifactHash(files) {
  const hash = createHash('sha256')
  for (const file of Object.keys(files).sort()) {
    hash.update(file)
    hash.update('\0')
    hash.update(files[file])
    hash.update('\0')
  }
  return hash.digest('hex')
}

function projectSlug(projectId) {
  const value = String(projectId || '').toLowerCase().replace(/[^a-z0-9]/g, '')
  return value.slice(-12) || createHash('sha256').update(String(projectId)).digest('hex').slice(0, 12)
}

function deterministicPort(projectId) {
  const digest = createHash('sha256').update(String(projectId)).digest()
  return 20000 + (digest.readUInt32BE(0) % 4000)
}

function publicHostname(projectId) {
  return `omni-${projectSlug(projectId)}.${publicIp}.nip.io`
}

async function loadState() {
  try {
    const parsed = JSON.parse(await readFile(stateFile, 'utf8'))
    return parsed && typeof parsed === 'object' ? parsed : {}
  } catch {
    return {}
  }
}

async function saveState(state) {
  await mkdir(rootDir, { recursive: true, mode: 0o700 })
  const temp = `${stateFile}.${process.pid}.tmp`
  await writeFile(temp, JSON.stringify(state, null, 2) + '\n', { mode: 0o600 })
  await rename(temp, stateFile)
}

async function updateJob(deployId, patch) {
  const state = await loadState()
  const next = {
    ...(state[deployId] || {}),
    ...patch,
    deployId,
    updatedAt: new Date().toISOString(),
  }
  state[deployId] = next
  await saveState(state)
  return next
}

function run(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: options.cwd,
      env: options.env || process.env,
      stdio: ['ignore', 'pipe', 'pipe'],
    })

    let stdout = ''
    let stderr = ''
    const max = options.maxOutput || 60_000
    const append = (target, chunk) => (target + chunk.toString()).slice(-max)

    child.stdout.on('data', chunk => { stdout = append(stdout, chunk) })
    child.stderr.on('data', chunk => { stderr = append(stderr, chunk) })

    let timer
    if (options.timeoutMs) {
      timer = setTimeout(() => {
        child.kill('SIGTERM')
        setTimeout(() => child.kill('SIGKILL'), 5000).unref()
      }, options.timeoutMs)
      timer.unref()
    }

    child.on('error', reject)
    child.on('close', code => {
      if (timer) clearTimeout(timer)
      if (code === 0) resolve({ stdout, stderr })
      else reject(new Error(`${command} exited ${code}: ${(stderr || stdout).slice(-5000)}`))
    })
  })
}

async function pickPort(projectId, deployId) {
  const state = await loadState()
  const occupied = new Set(
    Object.entries(state)
      .filter(([id, record]) => id !== deployId && record && record.status !== 'error')
      .map(([, record]) => Number(record.port))
      .filter(Number.isFinite),
  )

  const initial = deterministicPort(projectId)
  for (let i = 0; i < 4000; i += 1) {
    const candidate = 20000 + ((initial - 20000 + i) % 4000)
    if (!occupied.has(candidate)) return candidate
  }
  throw new Error('no_free_release_port')
}

function generatedDockerfile() {
  return [
    'FROM node:22-bookworm-slim',
    'WORKDIR /app',
    'ENV NEXT_TELEMETRY_DISABLED=1',
    'COPY package*.json ./',
    'RUN npm install --ignore-scripts --no-audit --no-fund --package-lock=false',
    'COPY . .',
    'RUN npm run build',
    'ENV NODE_ENV=production',
    'ENV PORT=3000',
    'EXPOSE 3000',
    'CMD ["npm","start"]',
    '',
  ].join('\n')
}

async function writeArtifact(workspace, files, envVars) {
  await rm(workspace, { recursive: true, force: true })
  await mkdir(workspace, { recursive: true, mode: 0o700 })

  for (const [relative, content] of Object.entries(files)) {
    if (typeof content !== 'string') throw new Error(`non_text_artifact:${relative}`)
    const target = safeArtifactPath(workspace, relative)
    await mkdir(path.dirname(target), { recursive: true, mode: 0o700 })
    await writeFile(target, content, { mode: 0o600 })
  }

  await writeFile(path.join(workspace, 'Dockerfile.omnicode'), generatedDockerfile(), { mode: 0o600 })

  const filtered = Object.entries(envVars || {})
    .filter(([key, value]) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(key) && typeof value === 'string')
    .slice(0, 100)

  if (filtered.length) {
    const envText = filtered
      .map(([key, value]) => `${key}=${String(value).replace(/\r?\n/g, '\\n')}`)
      .join('\n') + '\n'
    await writeFile(path.join(workspace, '.omnicode-runtime.env'), envText, { mode: 0o600 })
  }

  await writeFile(
    path.join(workspace, '.dockerignore'),
    ['node_modules', '.next', '.git', '.omnicode-runtime.env', 'Dockerfile*~', ''].join('\n'),
    { mode: 0o600 },
  )
}

async function installCaddyRoute(projectId, hostname, port) {
  const marker = projectSlug(projectId)
  const begin = `# BEGIN OMNICODE GENERATED ${marker}`
  const end = `# END OMNICODE GENERATED ${marker}`
  const block = [
    begin,
    `${hostname} {`,
    `    reverse_proxy 127.0.0.1:${port}`,
    '    encode zstd gzip',
    '}',
    end,
  ].join('\n')

  const original = await readFile(caddyFile, 'utf8')
  const start = original.indexOf(begin)
  let cleaned = original

  if (start >= 0) {
    const finish = original.indexOf(end, start)
    if (finish >= 0) {
      cleaned = original.slice(0, start) + original.slice(finish + end.length)
    }
  }

  const next = cleaned.trimEnd() + '\n\n' + block + '\n'
  const backup = `${caddyFile}.omnicode-${Date.now()}.bak`

  await copyFile(caddyFile, backup)
  await writeFile(caddyFile, next)

  try {
    await run('caddy', ['validate', '--config', caddyFile], { timeoutMs: 30_000 })
    await run('systemctl', ['reload', 'caddy'], { timeoutMs: 30_000 })
  } catch (error) {
    await copyFile(backup, caddyFile)
    await run('systemctl', ['reload', 'caddy'], { timeoutMs: 30_000 }).catch(() => undefined)
    throw error
  }
}

async function waitLocal(port) {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/`, {
        redirect: 'manual',
        signal: AbortSignal.timeout(5000),
      })
      if (response.status < 500) return
    } catch {}
    await new Promise(resolve => setTimeout(resolve, 2000))
  }
  throw new Error('container_health_timeout')
}

async function waitPublic(url) {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const response = await fetch(url, {
        redirect: 'manual',
        signal: AbortSignal.timeout(8000),
      })
      if (response.status < 500) return
    } catch {}
    await new Promise(resolve => setTimeout(resolve, 2000))
  }
  throw new Error('public_route_health_timeout')
}

async function deploy(payload, deployId) {
  const files = payload.files && typeof payload.files === 'object' && !Array.isArray(payload.files)
    ? payload.files
    : {}

  if (!payload.projectId || !payload.artifactHash || !Object.keys(files).length) {
    throw new Error('invalid_deploy_payload')
  }

  if (Object.keys(files).length > 500) throw new Error('too_many_artifact_files')

  const totalBytes = Object.values(files)
    .reduce((sum, content) => sum + Buffer.byteLength(String(content), 'utf8'), 0)
  if (totalBytes > 8 * 1024 * 1024) throw new Error('artifact_too_large')

  const actualHash = artifactHash(files)
  if (!safeEqual(actualHash, payload.artifactHash)) throw new Error('artifact_hash_mismatch')

  const slug = projectSlug(payload.projectId)
  const workspace = path.join(rootDir, slug, payload.artifactHash.slice(0, 16))
  const image = `omnicode-generated:${slug}-${payload.artifactHash.slice(0, 12)}`
  const container = `omni-app-${slug}`
  const port = await pickPort(payload.projectId, deployId)
  const hostname = publicHostname(payload.projectId)
  const url = `https://${hostname}`

  await updateJob(deployId, {
    status: 'building',
    projectId: payload.projectId,
    buildId: payload.buildId,
    artifactHash: payload.artifactHash,
    url,
    port,
    hostname,
    logs: 'Materializing validated artifact.',
  })

  await writeArtifact(workspace, files, payload.envVars || {})

  await updateJob(deployId, { logs: 'Building isolated production image.' })
  await run(
    'docker',
    ['build', '-f', 'Dockerfile.omnicode', '-t', image, '.'],
    { cwd: workspace, timeoutMs: 12 * 60 * 1000 },
  )

  await run('docker', ['rm', '-f', container], { timeoutMs: 30_000 }).catch(() => undefined)

  const runArgs = [
    'run', '-d',
    '--name', container,
    '--restart', 'unless-stopped',
    '--memory', '1536m',
    '--cpus', '1.5',
    '--pids-limit', '512',
    '--security-opt', 'no-new-privileges',
    '-p', `127.0.0.1:${port}:3000`,
  ]

  try {
    await readFile(path.join(workspace, '.omnicode-runtime.env'), 'utf8')
    runArgs.push('--env-file', path.join(workspace, '.omnicode-runtime.env'))
  } catch {}

  runArgs.push(image)

  await updateJob(deployId, { logs: 'Starting production container.' })
  await run('docker', runArgs, { timeoutMs: 60_000 })
  await waitLocal(port)

  await updateJob(deployId, { logs: 'Publishing HTTPS route.' })
  await installCaddyRoute(payload.projectId, hostname, port)
  await waitPublic(url)

  const inspect = await run(
    'docker',
    ['inspect', container, '--format', '{{.State.Status}}|{{.RestartCount}}|{{.State.OOMKilled}}'],
    { timeoutMs: 30_000 },
  )

  await updateJob(deployId, {
    status: 'ready',
    url,
    port,
    hostname,
    container,
    image,
    logs: `Validated artifact is live. container=${inspect.stdout.trim()}`,
    completedAt: new Date().toISOString(),
  })
}

async function queueDeployment(payload) {
  const requestedHash = String(payload.artifactHash || '')
  const projectId = String(payload.projectId || '')
  const stableId = `omni-vps-${projectSlug(projectId)}-${requestedHash.slice(0, 12)}`

  const state = await loadState()
  const existing = state[stableId]
  if (existing && ['building', 'ready'].includes(existing.status)) return existing

  const record = await updateJob(stableId, {
    status: 'building',
    projectId,
    buildId: String(payload.buildId || ''),
    artifactHash: requestedHash,
    logs: 'Deployment accepted by OmniCode VPS release agent.',
    createdAt: existing?.createdAt || new Date().toISOString(),
  })

  deploymentChain = deploymentChain
    .catch(() => undefined)
    .then(async () => {
      try {
        await deploy(payload, stableId)
      } catch (error) {
        await updateJob(stableId, {
          status: 'error',
          logs: error instanceof Error ? error.message.slice(-8000) : String(error).slice(-8000),
          completedAt: new Date().toISOString(),
        })
      }
    })

  return record
}

const server = createServer(async (req, res) => {
  try {
    if (req.url === '/health' && req.method === 'GET') {
      json(res, 200, { ok: true, service: 'omnicode-vps-release-agent' })
      return
    }

    if (!authorized(req)) {
      json(res, 401, { ok: false, error: 'unauthorized' })
      return
    }

    if (req.url === '/deploy' && req.method === 'POST') {
      const payload = await bodyJson(req)
      const record = await queueDeployment(payload)
      json(res, 202, {
        ok: true,
        deployId: record.deployId,
        status: record.status,
        url: record.url,
        logs: record.logs,
      })
      return
    }

    if (req.url?.startsWith('/status') && req.method === 'GET') {
      const parsed = new URL(req.url, `http://${listenHost}:${listenPort}`)
      const deployId = parsed.searchParams.get('deployId') || ''
      const state = await loadState()
      const record = state[deployId]
      if (!record) {
        json(res, 404, { ok: false, error: 'deployment_not_found' })
        return
      }
      json(res, 200, {
        ok: true,
        deployId,
        status: record.status,
        url: record.url,
        logs: record.logs,
        artifactHash: record.artifactHash,
      })
      return
    }

    json(res, 404, { ok: false, error: 'not_found' })
  } catch (error) {
    json(res, 500, {
      ok: false,
      error: error instanceof Error ? error.message.slice(0, 1000) : String(error).slice(0, 1000),
    })
  }
})

await mkdir(rootDir, { recursive: true, mode: 0o700 })
server.listen(listenPort, listenHost, () => {
  console.log('[omnicode-release-agent] listening', {
    host: listenHost,
    port: listenPort,
    rootDir,
    publicIp,
  })
})
