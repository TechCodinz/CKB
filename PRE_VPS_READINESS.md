# CKB Pre-VPS Readiness Gate

This checklist exists so infrastructure purchase is the final activation step, not the start of another engineering phase.

## Release independence
- Run `scripts/preflight-v13.sh` successfully on a clean machine.
- Package the current VS Code version into `ckb-vscode-<version>.vsix`.
- Record and retain the generated SHA-256 checksum.
- Install-test the exact VSIX artifact before Marketplace publication.
- Publish with Marketplace credentials supplied only at runtime; never commit tokens.
- Keep a previous known-good VSIX for rollback.

## Architecture truth
- Compare incremental graph updates against a clean full rescan on representative large repositories.
- Verify stable symbol identity through rename/move/refactor scenarios.
- Verify dependency/call graph, blast radius, failure cone and cache invalidation determinism.
- Test interrupted scan recovery and corrupted-cache recovery.

## Runtime truth
- Replay real OTLP workloads across multiple services.
- Verify source identity when functions share names across files/services.
- Verify parent/child span correlation and runtime-to-source mapping.
- Confirm STATIC never becomes RUNTIME without observed telemetry.
- Confirm stale/disappearing telemetry degrades truth classification rather than inventing activity.

## Protocol compatibility
- Exercise CLI -> MCP -> VS Code -> Cloud end to end.
- Exercise JetBrains -> MCP -> Cloud end to end.
- Exercise Antigravity/generic MCP client discovery and calls.
- Validate all public V13 JSON schemas against emitted payloads.

## Security and cost controls
- No secrets in source, logs, VSIX or generated diagnostics.
- Bound repo size, scan concurrency, request duration and memory-heavy operations.
- Rate-limit expensive endpoints before execution.
- Reject unauthenticated cloud operations by default.
- Preserve explicit Guarded Change validation before any source mutation.

## VPS activation gate
Purchase/provision only after all software-only gates above pass or have an explicit accepted exception. The VPS should then require only: OS hardening, Docker installation, secrets provisioning, DNS/TLS, self-hosted runner registration, deployment and smoke verification.
