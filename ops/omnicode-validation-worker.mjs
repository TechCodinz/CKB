#!/usr/bin/env node
import { createHmac } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { mkdir, rm, writeFile } from 'node:fs/promises'
import path from 'node:path'

const base = (process.env.OMNICODE_URL || 'https://omnicode-pro.vercel.app').replace(/\/$/, '')
const workerId = process.env.OMNICODE_VALIDATION_WORKER_ID || 'vps-validation-1'
const rootSecret = (process.env.OMNICODE_CKB_SHARED_SECRET || process.env.OMNICODE_CKB_FEEDBACK_SECRET || '').trim()
const pollMs = Math.max(5, Number(process.env.OMNICODE_VALIDATION_POLL_SECONDS || 15)) * 1000

if (!rootSecret) {
  console.error('[validation-worker] OMNICODE_CKB_SHARED_SECRET is required')
  process.exit(1)
}

const runnerSecret = createHmac('sha256', rootSecret)
  .update('omnicode-validation-runner-v1')
  .digest('hex')

const headers = {
  'content-type': 'application/json',
  'x-omnicode-validation-runner-secret': runnerSecret,
}

function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms))
}

async function post(route, body) {
  const response = await fetch(`${base}${route}`, {
    method: 'POST',
    headers,
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(45_000),
  })
  const text = await response.text()
  let payload = {}
  try { payload = text ? JSON.parse(text) : {} } catch { payload = { raw: text.slice(0, 500) } }
  if (!response.ok) {
    throw new Error(`${route} returned ${response.status}: ${JSON.stringify(payload).slice(0, 800)}`)
  }
  return payload
}

function safePath(root, relative) {
  if (!relative || relative.startsWith('/') || relative.includes('..') || relative.includes('\\')) {
    throw new Error(`Unsafe artifact path: ${relative}`)
  }
  const resolved = path.resolve(root, relative)
  if (!resolved.startsWith(path.resolve(root) + path.sep)) throw new Error(`Unsafe artifact path: ${relative}`)
  return resolved
}

async function fetchFile(job, filePath) {
  let offset = 0
  let content = ''
  for (;;) {
    const chunk = await post('/api/integrations/validation-runner/jobs/source', {
      runId: job.id,
      workerId,
      artifactHash: job.artifactHash,
      path: filePath,
      offset,
    })
    content += String(chunk.content || '')
    if (chunk.done || chunk.nextOffset == null) break
    offset = Number(chunk.nextOffset)
    if (!Number.isFinite(offset) || offset < 0) throw new Error('Invalid source pagination')
  }
  return content
}

function runDocker(workspace, args, timeout) {
  const name = `omni-val-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
  const result = spawnSync('docker', [
    'run', '--rm', '--name', name,
    '--memory=2g', '--cpus=2', '--pids-limit=256',
    '--security-opt=no-new-privileges',
    '-v', `${workspace}:/workspace`,
    '-w', '/workspace',
    ...args,
  ], {
    encoding: 'utf8',
    timeout,
    maxBuffer: 2 * 1024 * 1024,
    env: { PATH: process.env.PATH || '/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin' },
  })
  return {
    ok: result.status === 0 && !result.error,
    status: result.status,
    signal: result.signal,
    output: `${result.stdout || ''}\n${result.stderr || ''}`.slice(-12_000),
    error: result.error?.message,
  }
}

function commandArgs(plan) {
  if (!Array.isArray(plan) || !plan.length) return null
  const allowed = new Set(['npm', 'next', 'tsc', 'vitest', 'jest', 'vite'])
  if (!allowed.has(String(plan[0]))) throw new Error(`Command not allowlisted: ${plan[0]}`)
  return plan.map(value => String(value))
}

async function heartbeat(job) {
  return post('/api/integrations/validation-runner/jobs/heartbeat', {
    runId: job.id,
    workerId,
    artifactHash: job.artifactHash,
    leaseMinutes: 8,
  })
}

async function submit(job, payload) {
  return post('/api/integrations/validation-runner/jobs/result', {
    runId: job.id,
    workerId,
    artifactHash: job.artifactHash,
    ...payload,
  })
}

async function execute(job) {
  const workspace = path.join('/tmp', `omnicode-validation-${job.id}`)
  await rm(workspace, { recursive: true, force: true })
  await mkdir(workspace, { recursive: true, mode: 0o700 })

  const heartbeatTimer = setInterval(() => {
    heartbeat(job).catch(error => console.warn('[validation-worker] heartbeat:', error.message))
  }, 60_000)
  heartbeatTimer.unref()

  try {
    for (const item of job.manifest || []) {
      const filePath = String(item.path || '')
      const target = safePath(workspace, filePath)
      await mkdir(path.dirname(target), { recursive: true, mode: 0o700 })
      await writeFile(target, await fetchFile(job, filePath), { mode: 0o600 })
    }

    const policy = job.policy && typeof job.policy === 'object' ? job.policy : {}
    const plan = policy.commandPlan && typeof policy.commandPlan === 'object' ? policy.commandPlan : {}

    console.log(`[validation-worker] ${job.id} install (${job.manifest?.length || 0} files)`)
    const install = runDocker(workspace, [
      '--network', 'bridge',
      'node:22-bookworm-slim',
      'npm', 'install', '--ignore-scripts', '--no-audit', '--no-fund', '--package-lock=false',
    ], 240_000)

    if (!install.ok) {
      console.error('[validation-worker] dependency install failed:', install.output.slice(-2500))
      await submit(job, { outcome: 'infra_failed', errorCode: 'dependency_install_failed' })
      return
    }

    await heartbeat(job)

    const compileCmd = commandArgs(plan.compile)
    if (!compileCmd) throw new Error('Validation job has no compile command')
    const compileStarted = Date.now()
    const compile = runDocker(workspace, [
      '--network', 'none',
      'node:22-bookworm-slim',
      ...compileCmd,
    ], 330_000)
    const compileResult = {
      status: compile.ok ? 'passed' : 'failed',
      durationMs: Date.now() - compileStarted,
    }

    let testsResult = { status: 'skipped' }
    if (compile.ok) {
      const testsCmd = commandArgs(plan.tests)
      if (testsCmd) {
        await heartbeat(job)
        const testsStarted = Date.now()
        const tests = runDocker(workspace, [
          '--network', 'none',
          'node:22-bookworm-slim',
          ...testsCmd,
        ], 330_000)
        testsResult = {
          status: tests.ok ? 'passed' : 'failed',
          durationMs: Date.now() - testsStarted,
        }
        if (!tests.ok) console.error('[validation-worker] tests failed:', tests.output.slice(-2500))
      }
    }

    if (!compile.ok) console.error('[validation-worker] compile failed:', compile.output.slice(-2500))

    const result = await submit(job, {
      outcome: 'completed',
      compile: compileResult,
      tests: testsResult,
    })
    console.log('[validation-worker] completed', job.id, JSON.stringify({
      validationState: result.validationState,
      releaseReady: result.releaseReady,
      release: result.release?.state,
    }))
  } finally {
    clearInterval(heartbeatTimer)
    await rm(workspace, { recursive: true, force: true }).catch(() => undefined)
  }
}

async function main() {
  console.log('[validation-worker] started', { base, workerId, reconcileMode: 'claim-driven' })

  for (;;) {
    try {
      const payload = await post('/api/integrations/validation-runner/jobs/claim', {
        workerId,
        leaseMinutes: 8,
      })
      if (payload.reconcileScheduled) {
        console.log('[validation-worker] reconcile scheduled by OmniCode')
      }
      if (payload.job) await execute(payload.job)
      else await sleep(pollMs)
    } catch (error) {
      console.error('[validation-worker]', error instanceof Error ? error.message : String(error))
      await sleep(Math.max(pollMs, 15_000))
    }
  }
}

main().catch(error => {
  console.error('[validation-worker] fatal:', error)
  process.exit(1)
})
