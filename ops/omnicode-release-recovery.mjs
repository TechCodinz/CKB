// Recover only an already-built, artifact-bound container after a routing failure.
// Never rebuild, erase failures, or widen the deployment retry budget here.
export async function recoverExistingRoute(record, requestedHash, deps) {
  if (!record || !/^[a-f0-9]{64}$/.test(requestedHash) || record.artifactHash !== requestedHash) {
    throw new Error('recovery_artifact_mismatch')
  }
  if (record.status !== 'error' || !/^systemctl exited \d+:/.test(record.logs || '')) {
    throw new Error('recovery_requires_recorded_route_failure')
  }
  const slug = String(record.projectId || '').toLowerCase().replace(/[^a-z0-9]/g, '').slice(-12)
  const image = `omnicode-generated:${slug}-${requestedHash.slice(0, 12)}`
  const container = `omni-app-${slug}`
  if (!slug || record.deployId !== `omni-vps-${slug}-${requestedHash.slice(0, 12)}` ||
      !Number.isInteger(record.port) || record.port < 20000 || record.port >= 24000 ||
      !/^omni-[a-z0-9]+\.\d+\.\d+\.\d+\.\d+\.nip\.io$/.test(record.hostname || '') ||
      !record.hostname.startsWith(`omni-${slug}.`) || record.url !== `https://${record.hostname}`) {
    throw new Error('recovery_record_identity_mismatch')
  }
  const actual = await deps.inspect(container, image)
  if (!actual.running || actual.image !== image || !actual.imageId ||
      actual.imageId !== actual.expectedImageId ||
      !actual.bindings?.some(p => p.HostIp === '127.0.0.1' && p.HostPort === String(record.port))) {
    throw new Error('recovery_container_identity_mismatch')
  }
  await deps.checkLocal(record.port)
  await deps.installRoute(record.projectId, record.hostname, record.port)
  await deps.checkPublic(record.url)
  return {
    status: 'ready', artifactHash: requestedHash, container, image,
    completedAt: new Date().toISOString(),
    logs: 'Existing validated container recovered after local and public HTTP 200 checks.',
    routeRecovery: {
      previousError: record.logs,
      previousCompletedAt: record.completedAt || null,
      verifiedImageId: actual.imageId,
      recoveredAt: new Date().toISOString(),
      authority: 'authenticated-provider-route-recovery',
    },
  }
}
