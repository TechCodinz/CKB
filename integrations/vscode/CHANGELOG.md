# Changelog

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
