# CKB V13.2 Release Hardening — Verification Report

**Date:** 2026-08-12  
**Branch:** `agent/v13.2-memory-lane`  
**Engineer:** Antigravity (automated hardening pass)

---

## Summary

| Gate | Result |
|------|--------|
| `cargo check --workspace` | ✅ CLEAN |
| `cargo test --workspace` | ✅ **116 / 116 PASS** |
| VS Code `tsc` compile | ✅ CLEAN |
| VS Code VSIX package | ✅ `ckb-vscode-1.9.1.vsix` |
| Backend `prisma validate` | ✅ VALID |
| Backend `prisma generate` | ✅ OK |
| Backend `npm run build` (tsc) | ✅ CLEAN |
| Web `tsc --noEmit` | ✅ CLEAN |
| Web `react-scripts build` | ✅ **Compiled successfully** |
| Auth bypass check | ✅ NONE FOUND |

---

## Commit SHAs

| Repo | Branch | Commit |
|------|--------|--------|
| CKB Core / Extension | `agent/v13.2-memory-lane` | `bc3078e698ab008c9d2575977553a0d089a45ecb` |
| CKB Cloud | `main` | `76e18444a78bdf40d6a11f0aae181c47b67a04fd` |

---

## Phase 1 — Rust Core (`agent/v13.2-memory-lane`)

### Compiler Errors Fixed

| # | Error | File | Fix |
|---|-------|------|-----|
| 1 | `E0502` borrow: immutable borrow used while also mutably borrowed | `core/src/analysis/deep_causality_contract_fields.rs:88` | Separated `find().cloned()` into separate `let` before calling `upsert_entity()` |
| 2 | `E0502` borrow (×2) | `core/src/analysis/deep_causality_bundle.rs:85,87` | Same borrow separation fix in `merge_deep_causality_evidence()` |
| 3 | `E0603` private module × 5 | `cli/src/bin/ckb-memory-lane.rs` | Changed `ckb_core::analysis::X` → `ckb_core::X` (items already re-exported from crate root) |
| 4 | `E0603` private module × 5 | `cli/src/bin/ckb-causality.rs` | Same fix |
| 5 | `E0603` private module | `mcp-server/src/bin/memory_lane_mcp.rs` | Same fix |
| 6 | `E0603` private module | `mcp-server/src/bin/deep_causality_mcp.rs` | Same fix |
| 7 | `E0277` collect() type inference × 6 | `mcp-server/src/bin/memory_lane_mcp.rs:26` | Added explicit `Vec<String>` type annotation |

### Warnings Cleaned

| # | Warning | File | Fix |
|---|---------|------|-----|
| 8 | `unused_imports` | `core/src/analysis/mod.rs:37` | Added `#[allow(unused_imports)]` on `deep_causality_advanced::*` re-export |
| 9 | unused import `Path` | `mcp-server/src/bin/reality_gateway.rs:15` | Removed `Path` from import |
| 10 | `dead_code` × 6 fields | `mcp-server/src/bin/reality_bridge.rs` | Added `#[allow(dead_code)]` on 6 forward-compat deserialization struct fields (`project_id`, `repo_name`) |

### Test Errors Fixed

| # | Error | File | Fix |
|---|-------|------|-----|
| 11 | 12 type errors (stale API names) | `core/tests/v13_intelligence_contract.rs` | Rewrote entire test to match current API: `ArchitectureTaskKind`, `ArchitectureMemorySlice`, `MemoryEvidence`, `MemoryRetrievalStats`, correct `EvolutionProposal` fields, `PromotionDecision` (struct, not enum) |
| 12 | CRLF assertion failure (Windows) | `core/src/vcs/patch_transaction.rs` (×2) | Added `.replace("\r\n", "\n")` before assertion |

### Test Results

```
test result: ok. 112 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.64s

Running tests\v13_intelligence_contract.rs

running 4 tests
test production_self_evolution_requires_explicit_promotion_and_validation ... ok
test compiled_context_never_promotes_predicted_provenance_to_runtime ... ok
test public_aql_is_deterministic_and_model_neutral ... ok
test ambiguous_otlp_identity_is_not_a_source_symbol ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

**Total: 116 / 116 tests pass, 0 failed.**

### Preserved Capabilities ✅

- Project-bounded Memory Lane (`MemoryLaneStore`, `MemoryLaneEpisode`)
- Episodic / semantic / procedural / runtime / preference / reflection memory
- Strategy outcome learning (`LearningOutcome`)
- Risk / retrieval adaptation
- Checkpoints and restore (`store.checkpoint()`, `store.save()`)
- Causal snapshot observation (`observe_causal_snapshot`)
- MCP tools (`memory_lane_mcp`, `deep_causality_mcp`, `reality_gateway`, `reality_bridge`)
- Guarded Change requirement (`PromotionGate::evaluate()`, `EvolutionProposal`)

---

## Phase 2 — VS Code Extension

### Compiler Error Fixed

| # | Error | File | Fix |
|---|-------|------|-----|
| 13 | `TS2339`: Property `item` does not exist on type `string` | `src/modelIntelligenceV13.ts:281` | Added `ModelRegistryPickItem extends vscode.QuickPickItem { item: any }` interface; typed `showQuickPick<ModelRegistryPickItem>()` call |

### VSIX Artifact

| Field | Value |
|-------|-------|
| Filename | `ckb-vscode-1.9.1.vsix` |
| Size | 333.92 KB (19 files) |
| SHA256 | `0A13CAC2C39881E1AC062D8ADBBD0288F66C9CB473D67574B2EEE349F9FE9309` |
| Entry point | `out/extensionV6.js` |

---

## Phase 3 — CKB Cloud Backend + Web

### Backend

| Step | Result |
|------|--------|
| `npm ci` | ✅ 207 packages, EXIT:0 |
| `prisma validate` | ✅ Schema valid |
| `prisma generate` | ✅ Prisma Client v5.22.0 generated |
| `npm run build` (tsc) | ✅ EXIT:0, no type errors |

### Web Frontend

| Step | Result |
|------|--------|
| `npm ci` | ✅ 1469 packages, EXIT:0 |
| `tsc --noEmit` | ✅ EXIT:0, zero type errors |
| `react-scripts build` | ✅ **Compiled successfully** — `820.52 kB` gzipped bundle |

> **Note:** The webpack production build requires `GENERATE_SOURCEMAP=false` and `NODE_OPTIONS=--max-old-space-size=4096` on this machine due to large bundle size. This is a local resource constraint — CI/Vercel builds are not affected.

### Auth Security Check

```
Patterns checked: demo_token, fake.*token, bypass.*auth, skip.*auth
Routes scanned: analyze.ts, oauth.ts, pricing.ts, raiziom.ts, raiziomChange.ts,
                reality.ts, transactionLedger.ts
Result: NO matches found ✅
```

---

## Open Items (Action Required by Team)

> [!IMPORTANT]
> **Git Push:** `git push origin agent/v13.2-memory-lane` — requires GitHub auth (PAT/SSH).
> All commits are ready locally.

> [!NOTE]
> **Bundle size:** Web bundle is 820 kB gzipped, above CRA's 500 kB recommendation.
> Consider code-splitting `ProjectView.tsx` and lazy-loading heavy routes in a follow-up PR.

> [!NOTE]
> **npm audit:** Backend has 12 vulnerabilities (1 critical in passport-saml@3.2.4), web has 55.
> Run `npm audit fix` for non-breaking fixes. Breaking dep upgrades should be a separate PR.

---

## Do NOT Merge Automatically

- Branch `agent/v13.2-memory-lane` → `main` requires human PR review
- No automatic merges were performed
- All changes are compiler-verified and test-verified
