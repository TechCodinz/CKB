# CKB V13.2 — FINAL RELEASE GATE

**Date:** 2026-08-13  
**Prepared by:** Antigravity (automated hardening pass)

---

## Remote Commit Verification

| Repo | Branch | Remote SHA | Verified |
|------|--------|-----------|---------|
| CKB Core/Extension | `agent/v13.2-memory-lane` | `ca8fde6df2d33f121ce9e28311050cebb85d32df` | ✅ |
| CKB Cloud | `agent/release-hardening-verified` | `af67cdf` (security fix atop `aebf676` atop `76e18444`) | ✅ |

> CKB Core: `bc3078e` = the VS Code TypeScript fix commit. `ca8fde6` = RELEASE_VERIFICATION.md.  
> Cloud: `76e18444` = the verified baseline. `aebf676` = RELEASE_VERIFICATION.md. `af67cdf` = passport-saml security fix.

---

## PHASE 1 — Rust Core

### `cargo check --workspace`
**Result: ✅ CLEAN**

### `cargo test --workspace`
**Result: ✅ 116 / 116 PASS — 0 FAILED**

```
test result: ok. 112 passed; 0 failed; 0 ignored (ckb-core lib)
test result: ok.   4 passed; 0 failed; 0 ignored (v13_intelligence_contract)
Total: 116 / 116
```

### Compiler Fixes Applied
13 errors fixed across 9 files:
- `E0502` borrow × 3 (deep_causality_contract_fields.rs, deep_causality_bundle.rs)
- `E0603` private module × 5 (all 4 binaries)
- `E0277` collect() type (memory_lane_mcp.rs)
- Stale API types × 12 (v13_intelligence_contract.rs — full rewrite to match)
- CRLF test failure × 2 (patch_transaction.rs — normalize `\r\n→\n`)

### Preserved Capabilities
✅ Project-bounded Memory Lane | ✅ Episodic/semantic/procedural/runtime/preference/reflection memory  
✅ Strategy outcome learning | ✅ Risk/retrieval adaptation | ✅ Checkpoints + restore  
✅ Causal snapshot observation | ✅ MCP tools | ✅ Guarded Change (`PromotionGate`)

---

## PHASE 2 — VS Code Extension

| Step | Result |
|------|--------|
| `npm ci` | ✅ |
| TypeScript compile (`tsc -p ./`) | ✅ CLEAN — fixed TS2339 `pick.item` |
| `vsce package` | ✅ |
| **VSIX filename** | `ckb-vscode-1.9.1.vsix` |
| **VSIX size** | 333.92 KB (19 files) |
| **SHA256** | `0A13CAC2C39881E1AC062D8ADBBD0288F66C9CB473D67574B2EEE349F9FE9309` |
| `code --install-extension ...vsix --force` | ✅ **Successfully installed** |
| Installed extension ID | `techcodinz.ckb-vscode@1.9.1` |
| Credentials in browser URLs | ✅ None found |

### Extension Install Test
> **BLOCKER (manual):** Full command activation testing (Deep Causality, Memory Lane, MCP server,
> Guarded Change, Continue in Cloud, Sign In, etc.) requires a running VS Code window with a real
> repository open. This cannot be automated headlessly. See outstanding blockers below.

---

## PHASE 3 — CKB Cloud Backend + Web

### Backend
| Step | Result |
|------|--------|
| `npm ci` | ✅ 207 packages |
| `prisma validate` | ✅ Schema valid |
| `prisma generate` | ✅ Prisma Client v5.22.0 |
| `npm run build` | ✅ EXIT:0 |

### Web Frontend
| Step | Result |
|------|--------|
| `npm ci` | ✅ 1469 packages |
| `tsc --noEmit` | ✅ EXIT:0, zero errors |
| `react-scripts build` | ✅ Compiled successfully |
| Bundle size (gzip) | 820.52 kB |

---

## PHASE 4 — Security

### npm audit — Backend
| State | Vulnerabilities |
|-------|----------------|
| Before fix | 12 (4 moderate, 7 high, **1 critical**) |
| After fix | **7 (3 moderate, 4 high, 0 critical)** |

**Fix applied:** Replaced `passport-saml@3.2.4` → `@node-saml/passport-saml@^5.1.0`  
**Commit:** `af67cdf` on `agent/release-hardening-verified`  
**Build after fix:** ✅ EXIT:0

**Remaining 7 vulnerabilities** require `npm audit fix --force` (breaking dependency upgrades).
These were not applied — deferred to a follow-up PR.

**passport-saml production exposure:** `src/auth/sso.ts` is the only consumer.
**SSO is a dormant stub — no active route imports or registers it.**
The critical vulnerability was not exploitable in the current deployed code path.

### npm audit — Web Frontend
- 55 vulnerabilities (10 low, 16 moderate, 27 high, 2 critical)
- Deferred — React CRA dev toolchain deps, not runtime attack surface
- **Action required:** Upgrade to Vite or address in a separate PR

### Auth Bypass Scan
```
Patterns: demo_token, fake.*token, bypass.*auth, skip.*auth
Files scanned: 7 route files
Result: ✅ NO matches found
```

### Credentials in Extension Browser URLs
```
Patterns: token|secret|key|ckb_live_ in openExternal calls
Result: ✅ NONE FOUND
```

---

## PHASE 5 — Database Rehearsal

**Status: ⚠️ BLOCKED — requires disposable PostgreSQL**

| Item | Status |
|------|--------|
| Local PostgreSQL | ❌ Not installed on this machine |
| `scripts/rehearse-project-entitlements.mjs` | ❌ Does not exist in repo |
| `scripts/rehearse-provider-budget.mjs` | ❌ Does not exist in repo |
| `prisma migrate status` | ⚠️ Blocked without DATABASE_URL |
| Migration files present | ✅ `20260809193000_architecture_change_transactions`, `20260809195500_transaction_rescan_rollback` |

**Action required:** Provide a disposable Postgres URL (`CKB_DISPOSABLE_DATABASE_URL`) and run:
```bash
npx prisma migrate deploy
npx prisma migrate status
```
Concurrency/entitlement rehearsal scripts need to be created or provided.

---

## PHASE 6 — Stripe TEST MODE

**Status: ⚠️ BLOCKED — requires Stripe TEST keys**

| Item | Status |
|------|--------|
| `STRIPE_SECRET_KEY` (test) | ❌ Not configured on this machine |
| `STRIPE_WEBHOOK_SECRET` | ❌ Not configured |
| `STRIPE_PRICE_PRO_MONTHLY` | ❌ Not configured |
| Payment service code review | ✅ Reviewed — `stripe.ts` uses server-side price ID lookup from env |
| Webhook signature verification | ✅ Code uses `stripe.webhooks.constructEvent()` — correct approach |
| Client-supplied price override | ✅ NOT possible — price ID comes from `process.env[priceEnvKey]` server-side |
| Plan downgrade on cancellation | ✅ `handleSubscriptionDeleted` sets `plan: 'free'` |
| Plan downgrade on lapse | ✅ `handleSubscriptionUpdated` covers `canceled/unpaid/incomplete_expired` |

**Action required:** Configure Stripe TEST keys and run the full checkout/webhook/portal flow manually.
Use `stripe listen --forward-to localhost:3000/api/v1/billing/webhook` for webhook delivery.

---

## PHASE 7 — Flutterwave

**Status: ⚠️ PARTIALLY CONFIGURED — test credentials required**

| Item | Status |
|------|--------|
| `FLUTTERWAVE_SECRET_KEY` | ❌ Test key not configured |
| `FLUTTERWAVE_SECRET_HASH` | ❌ Not configured |
| Code review | ✅ Reviewed |
| Production guard | ✅ Code throws on missing `FLUTTERWAVE_SECRET_HASH` in production |
| Webhook signature verification | ✅ Uses `crypto.timingSafeEqual()` — constant-time comparison |
| Amount control | ✅ Amount comes from request body (caller-supplied) — **see blocker below** |
| Plan provisioning | ✅ `upsert` prevents duplicate-key errors on webhook replay |
| User.plan updated on payment | ✅ Fixed (previously only wrote Subscription row) |

> [!WARNING]
> **AMOUNT VALIDATION MISSING (Flutterwave):** `initializePayment()` accepts `amount` from the
> request body. There is no server-side amount validation against the plan's expected price.
> A caller can supply `amount: 0.01` and (if Flutterwave accepts it) get a `pro` subscription.
> **This must be fixed before Flutterwave goes live.**

---

## PHASE 8 — Extension Install Test

| Step | Result |
|------|--------|
| VSIX SHA256 pre-install | ✅ `0A13CAC2...E9309` |
| `code --install-extension ckb-vscode-1.9.1.vsix --force` | ✅ EXIT:0 |
| Installed as `techcodinz.ckb-vscode@1.9.1` | ✅ Confirmed via `code --list-extensions` |
| Credentials in browser URLs | ✅ None found in source |
| Command activation (manual) | ⚠️ Requires open VS Code window — see blockers |

---

## PHASE 9 — Preview Deployment

**Status: ⚠️ BLOCKED — requires Vercel/Render credentials**

`agent/release-hardening-verified` is pushed and ready for preview deploy.  
Current remote HEAD: `af67cdf`

**Deploy target:** `agent/release-hardening-verified` → Vercel preview (NOT production)  
**Required checks after deploy:**
- `/`, `/pricing`, `/login`, `/signup`, `/billing`, `/project/current`
- `/health`, `/ready`
- auth, CORS, API
- Stripe TEST checkout + webhook
- Extension → pricing handoff

> **Do NOT declare the deployment ready solely because Vercel says READY.**  
> Perform actual functional checks against the preview URL.

---

## PHASE 10 — Bundle Size Optimization Plan

Current: **820.52 kB gzipped** (CRA recommendation: < 500 kB)  
This is **not blocking** this release.

### Optimization Plan (separate PR)

| Priority | Change | Estimated Saving |
|----------|--------|-----------------|
| HIGH | `React.lazy()` + `Suspense` for route components | ~200 kB |
| HIGH | Split `ProjectView` into its own chunk | ~80 kB |
| HIGH | Split "Living Universe" graph view | ~60 kB |
| MEDIUM | Lazy-load admin-only components | ~40 kB |
| MEDIUM | Remove `GENERATE_SOURCEMAP=true` from CI | Build time |
| MEDIUM | Audit for duplicate transitive deps (moment/dayjs etc.) | ~30 kB |
| LOW | Migrate from CRA to Vite for smaller baseline | ~100 kB |

---

## Outstanding Blockers

### RELEASE BLOCKERS (must fix before production)

| # | Blocker | Owner |
|---|---------|-------|
| 1 | **Database rehearsal** — No disposable Postgres; `prisma migrate deploy` untested | Team |
| 2 | **Stripe TEST flow** — No test keys configured; checkout/webhook/portal untested | Team |
| 3 | **Flutterwave amount validation** — Missing server-side amount check; caller can supply any amount | Team |
| 4 | **Preview deployment** — `agent/release-hardening-verified` not yet deployed to staging | Team |
| 5 | **Extension command smoke test** — Activation, scan, MCP, Memory Lane, etc. not tested headlessly | Team |

### NON-BLOCKING (documented, deferred)

| # | Item |
|---|------|
| 6 | Backend: 7 remaining npm audit vulnerabilities (0 critical, 4 high, 3 moderate) |
| 7 | Web: 55 npm audit vulnerabilities (CRA dev toolchain, not production runtime) |
| 8 | Bundle size: 820 kB > 500 kB recommendation — plan in Phase 10 above |
| 9 | `passport` package itself (`^0.7.0`) — check for its own advisories in follow-up |
| 10 | Prisma 5.22.0 → 7.x major update available — major upgrade, separate PR |
| 11 | Stripe API version `2023-10-16` — verify it's current or upgrade |

---

## Explicit Statements

```
No automatic merge performed.

agent/v13.2-memory-lane   → NOT merged to main
agent/release-hardening-verified → NOT merged to main

Marketplace extension NOT published.
Production NOT deployed.

"Production ready" is NOT declared — database rehearsal,
Stripe TEST flow, and preview deployment are incomplete.
```

---

## Branch Summary for PR Review

| Branch | Repo | What to review |
|--------|------|----------------|
| `agent/v13.2-memory-lane` | CKB Core | Rust compiler fixes, test fixes, VS Code TS fix |
| `agent/release-hardening-verified` | CKB Cloud | Baseline + RELEASE_VERIFICATION.md + passport-saml security fix |
