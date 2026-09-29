import test from 'node:test'
import assert from 'node:assert/strict'
import { recoverExistingRoute } from '../omnicode-release-recovery.mjs'

const artifactHash = 'a'.repeat(64)
const record = { deployId: 'omni-vps-ic04hangjte5-aaaaaaaaaaaa', projectId: 'cmujzan4e000aic04hangjte5', artifactHash, status: 'error', logs: 'systemctl exited 1: reload failed', port: 22965, hostname: 'omni-ic04hangjte5.169.58.175.192.nip.io', url: 'https://omni-ic04hangjte5.169.58.175.192.nip.io', completedAt: '2026-09-28T15:48:20Z' }
function harness(overrides = {}) {
  const calls = []
  const deps = {
    inspect: async () => ({ running: true, image: 'omnicode-generated:ic04hangjte5-aaaaaaaaaaaa', imageId: 'sha256:exact', expectedImageId: 'sha256:exact', bindings: [{ HostIp: '127.0.0.1', HostPort: '22965' }] }),
    checkLocal: async () => calls.push('local'),
    installRoute: async () => calls.push('route'),
    checkPublic: async () => calls.push('public'),
    ...overrides,
  }
  return { calls, deps }
}
test('verifies the existing artifact and both routes before producing readiness evidence', async () => {
  const { calls, deps } = harness()
  const result = await recoverExistingRoute(record, artifactHash, deps)
  assert.deepEqual(calls, ['local', 'route', 'public'])
  assert.equal(result.status, 'ready')
  assert.equal(result.artifactHash, artifactHash)
  assert.equal(result.routeRecovery.previousError, record.logs)
  assert.equal(result.routeRecovery.previousCompletedAt, record.completedAt)
})
test('rejects a different artifact without touching the route', async () => {
  const { calls, deps } = harness()
  await assert.rejects(recoverExistingRoute(record, 'b'.repeat(64), deps), /artifact/)
  assert.deepEqual(calls, [])
})
test('does not recover build failures or unfinished deployments', async () => {
  for (const patch of [{ logs: 'npm build failed' }, { status: 'building' }]) {
    const { calls, deps } = harness()
    await assert.rejects(recoverExistingRoute({ ...record, ...patch }, artifactHash, deps), /route_failure/)
    assert.deepEqual(calls, [])
  }
})
test('rejects an image tag whose immutable image identity changed', async () => {
  const { calls, deps } = harness({ inspect: async () => ({ running: true, image: 'omnicode-generated:ic04hangjte5-aaaaaaaaaaaa', imageId: 'sha256:old', expectedImageId: 'sha256:new', bindings: [{ HostIp: '127.0.0.1', HostPort: '22965' }] }) })
  await assert.rejects(recoverExistingRoute(record, artifactHash, deps), /container_identity/)
  assert.deepEqual(calls, [])
})
test('public health failure never returns ready', async () => {
  const { deps } = harness({ checkPublic: async () => { throw Error('public_health_failed') } })
  await assert.rejects(recoverExistingRoute(record, artifactHash, deps), /public_health_failed/)
})
