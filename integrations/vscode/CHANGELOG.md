# Changelog

## 1.10.2

First-run and connectivity failures now explain themselves instead of surfacing raw Node error codes.

- **Fixed:** a fresh install with no CKB CLI and no configured server reported `connect ECONNREFUSED 127.0.0.1:3000`. CKB now states that no analysis backend is available and names the two ways to fix it, in a single notification rather than two competing ones.
- **Fixed:** connection failures are translated by cause — server unreachable, address unresolvable, TLS not verifiable, request timed out (including free-tier cold starts), authentication required, and connection reset — each naming the URL that was tried.
- **Fixed:** a `401` now distinguishes "no API key stored" from "the stored key was rejected", and points at the *CKB: Set API Key* command.

## 1.10.0

CKB V13.2 marketplace launch — the Invisible Reality architecture-intelligence experience is now aligned with the verified V13/V13.1/V13.2 core and Cloud release.

- **Added:** Invisible Reality V13 semantic editor intelligence with cursor-driven LINE/CALL/SYMBOL/FILE/SUBSYSTEM/SYSTEM depth.
- **Added:** Deep Software Causality workflows for data flow, taint, schema, infrastructure, concurrency, temporal, ownership and change-simulation analysis.
- **Added:** Raiziom-grounded in-IDE architecture questions and model-neutral architecture-context compilation.
- **Added:** Guarded Change Reality with prepare, validate, commit and rollback actions for evidence-backed workspace changes.
- **Added:** bidirectional Cloud continuity between VS Code and the CKB Living Universe.
- **Added:** verified frontier-model catalog, request compatibility checks and observed-model registry actions.
- **Added:** architecture memory, deep activity analysis, shareable reality snapshots and product-guidance milestones.
- **Changed:** Marketplace release is labeled as a trial experience, with paid Pro and Team capabilities continuing through CKB Cloud.
- **Hardened:** local/static/runtime evidence boundaries, Cloud context handling, API-key storage paths and guarded-change validation.

## 1.1.0

Fixes found during a pre-launch marketplace readiness review — none of
these were cosmetic:

- **Fixed:** the extension never sent an API key to the CKB server, so any
  server with authentication enabled (the recommended production
  configuration) silently rejected every request.
- **Fixed:** the default `ckb.serverUrl` pointed at an external hosted
  domain and was used as a fallback that sent your *local* workspace path
  over HTTP — a remote server can't read a path that only exists on your
  machine, so this fallback could never actually work. Default changed to
  `http://localhost:3000`, matched to what actually works (your own local
  server, if you run one).
- **Fixed:** `CKB: Check Architecture` used `--strict`, which correctly
  exits with a non-zero code when it finds violations — but the extension
  treated *any* non-zero exit as "the CLI isn't installed" and silently
  fell back to a broken remote path instead of using the real (successful)
  results already sitting in stdout.
- **Fixed:** violations on non-file-level nodes (functions, classes,
  methods) were mapped to an invalid file path and silently dropped instead
  of showing up as a diagnostic.
- **Fixed:** file-save-triggered re-analysis was a no-op — it spun the
  status bar icon for two seconds and did nothing else, despite being
  documented as a real feature. Now performs a real debounced re-scan.
- **Fixed:** `CKB: Check Architecture` reported "✅ Architecture compliant"
  on *any* error, including ones that meant the check never actually ran.
- **Added:** `ckb.apiKey` setting.
- **Added:** `ckb.rescanOnSave` setting to control the new debounced re-scan
  behavior independently from `ckb.autoScanOnOpen`.
- **Changed:** a missing CLI + unreachable server now shows one clear
  notification with an install-instructions link, instead of failing
  silently with only a console warning.

## 1.0.0

Initial release: scan, check, impact analysis, inline diagnostics, MCP
server integration.
