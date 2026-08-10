# CKB Architecture Intelligence Fabric V13

CKB V13 makes architecture intelligence the durable layer beneath rapidly changing AI models. A model is a consumer of CKB evidence, not the owner of repository truth.

## Core invariant

CKB must remain useful when a provider ships a new model tomorrow. Provider/model identity is metadata. It never changes whether evidence is STATIC, RUNTIME, PREDICTED, HISTORY, HUMAN, or VALIDATION evidence.

The system flow is:

```text
repository / telemetry / git / contracts / tests
                    ↓
         canonical CKB architecture memory
                    ↓
             evidence ledger
                    ↓
       task-specific context compiler
                    ↓
       model / agent / IDE / MCP client
                    ↓
              proposal(s)
                    ↓
   simulation → validation → guarded promotion
                    ↓
        post-change rescan / retrace
                    ↓
         observed evaluation ledger
                    ↓
      future model/task selection evidence
```

## Truth classes

- **STATIC** — AST/source/dependency facts. Static reachability is not execution.
- **RUNTIME** — exact observed telemetry such as OTLP traces and runtime metrics.
- **PREDICTED** — blast-radius simulation, forecasts, hypothetical architecture and other what-if results.
- **HISTORY** — persisted snapshots and Git/architecture evolution evidence.
- **HUMAN** — explicit intent, decisions and approvals.
- **VALIDATION** — compiler, tests, contracts, security checks and other executed validation evidence.

A high confidence score never promotes PREDICTED evidence into RUNTIME evidence.

## Architecture Constitution

V13 ships a machine-readable constitution with blocking invariants:

1. Runtime claims require observed telemetry.
2. Predictions remain hypotheses until independently validated or observed.
3. Architecture claims retain provenance.
4. Prefer repository-relative paths, stable symbol IDs and source spans.
5. Reasoning/proposal operations never silently mutate, merge, push or deploy source.
6. Production-affecting changes require appropriate validation before promotion.
7. Model context is bounded and task-specific; secrets and unrelated source are not exposed merely because a model has a large context window.
8. Self-evolution is guarded by the same evidence, validation and explicit promotion requirements as ordinary source changes.

Implementation: `core/src/analysis/intelligence_fabric.rs`.

## Context Compiler

`ContextCompiler` consumes a bounded `ArchitectureMemorySlice`, an architecture task, a model capability profile and a context budget. It emits `ckb-context-v1` containing:

- truth contract and task guidance,
- exact architecture roots,
- prioritized symbols and source coordinates,
- bounded relationships,
- observed runtime metrics only where attached,
- deduplicated evidence ledger,
- truncation metadata,
- the Architecture Constitution,
- informational model capability metadata.

Provider/model names do not change the evidence sections. They exist so an orchestrator can choose transport and budget behavior without teaching CKB a vendor-specific truth model.

Legacy boolean model metadata now fails closed: an omitted capability flag is not treated as support. Rich V2 support-state metadata remains the authoritative representation for provider compatibility.

## Frontier Model Capability Profile V2

The earlier boolean-only model profile remains accepted for compatibility, but the V13 contract now represents fast-changing provider behavior without pretending `false` means `unknown`.

The canonical schema is `schemas/model-capability-profile.schema.json`. It can represent:

- provider/model identity and aliases,
- GA / preview / limited / deprecated / retired lifecycle,
- release and verification timestamps plus freshness horizon,
- context-window and max-output limits,
- knowledge cutoff metadata,
- input/output modalities,
- supported API surfaces and preferred surface,
- reasoning modes, defaults, adaptive/always-on behavior and manual-budget support,
- explicit support states for function calling, structured output, parallel tools, code execution, computer use and MCP,
- named provider tools,
- deprecated, ignored, rejected and unsupported request parameters,
- unsupported turn patterns such as response prefilling,
- parameter migrations,
- tokenizer migration notes,
- primary-source provenance.

Support is a six-state value: `supported | unsupported | preview | beta | limited | unknown`. **Unknown stays unknown.** CKB does not infer a capability because a neighboring model has it.

Rust implementation: `core/src/analysis/frontier_model_profile.rs`.

## Provider lifecycle truth is separate from capability truth

Deprecation and retirement evidence has its own contract: `schemas/model-lifecycle-profile.schema.json` and the lifecycle structs in `core/src/analysis/frontier_model_profile.rs`.

A lifecycle record can describe:

- provider-documented state: active / legacy / preview / deprecated / retired / shutdown-scheduled / shutdown / unknown,
- deprecation date,
- retirement/shutdown date,
- whether that date is exact, tentative, or an earliest-possible date,
- recommended replacement,
- verification timestamp,
- exact primary-source reference.

CKB deliberately does **not** infer a provider state from the calendar. If a provider still labels a model `deprecated` after a tentative retirement date passes, CKB records `deprecated` until fresh provider evidence says otherwise. This avoids manufacturing lifecycle truth from a date.

Lifecycle-only evidence also does not invent capabilities. CKB can know “this model is retired; migrate to X” while leaving its historical context/tool support unknown.

## Primary-source verified catalog

CKB Cloud V13 ships a small bundled verified catalog as a safe bootstrap and also supports a trusted dynamic store. This prevents the intelligence architecture from requiring a code rewrite every time a provider introduces a new model or changes request semantics.

The effective capability catalog is:

```text
bundled verified bootstrap
          +
trusted dynamic verified store
          ↓
exact provider/model overlay
          ↓
IDE / MCP / agent capability consumers
```

Provider lifecycle evidence is returned beside capability profiles rather than being fabricated into them.

The dynamic capability store can only be updated through the dual-auth trusted sync path:

```text
POST /api/v1/mcp/architecture/internal/frontier-models/sync
Authorization: Bearer ckb_live_...
X-CKB-Internal-Secret: ...
```

A sync entry is rejected unless it has a valid model identity, verification timestamp, lifecycle state and at least one `official-doc`, `official-release`, or `provider-api` source. The profile is hashed before persistence. Malformed or hash-mismatched database records never override the bundled bootstrap catalog.

The read-only developer endpoints are:

```text
GET  /api/v1/mcp/architecture/models/catalog
POST /api/v1/mcp/architecture/models/request-adapt
```

`request-adapt` performs only data-driven compatibility transformations explicitly allowed by the verified profile. It can remove parameters documented as ignored/deprecated and report incompatible fields or turn patterns. It **does not execute the provider request**.

If CKB has lifecycle evidence but no capability profile, it returns migration/lifecycle truth and refuses to invent a compatibility transformation. Retired/shutdown lifecycle-only models return an unavailable response with the provider-documented replacement when one exists.

Freshness and lifecycle gate selection: stale, deprecated and retired verified profiles remain visible for history/migration inspection but cannot be attached as automated execution hints or become recommendation-eligible.

## IDE parity

VS Code and JetBrains V13 consume the same Cloud catalog. Both can:

- compile architecture context at the cursor,
- stay model-neutral or attach a fresh/selectable verified model capability hint,
- inspect the verified frontier catalog,
- inspect observed model/task validation history,
- inspect the Architecture Constitution,
- check a JSON provider request against verified compatibility metadata without executing it.

Deprecated, retired and stale records remain inspectable for migration work but are kept out of model-context selection paths.

No IDE uploads arbitrary source contents merely to choose a model profile.

## Snapshot-stable Raiziom reasoning

A grounded Raiziom turn is pinned to an exact CKB architecture snapshot **before** evidence retrieval. After optional model reasoning completes, CKB re-reads the graph snapshot. If the architecture advanced while the turn was in progress, the response is rejected with a stale-snapshot conflict instead of presenting an old answer as CURRENT reality.

The same turn therefore has:

```text
start snapshot
  ↓
retrieve bounded evidence
  ↓
compile CKB context + constitution
  ↓
optional Raiziom/model reasoning
  ↓
re-read snapshot
  ↓
return only if start == end
```

Provider sampling parameters are not imposed generically. A provider-owned adapter/configuration must explicitly opt in to parameters such as temperature after compatibility is known.

## Observed evaluation engine

An `EvaluationObservation` records only checks that actually ran. Missing checks remain `null`; CKB does not convert absence into success.

Supported outcome dimensions include:

- compile/build passed,
- tests passed,
- contracts passed,
- security checks passed,
- runtime regression observed,
- rollback required,
- exact validation references.

`ModelScorecard` aggregates recorded outcomes by task/provider/model. It is historical evidence, not a universal benchmark or a prediction of future performance.

The observed model registry does **not** rank an unobserved new frontier model merely because its context window or tool list looks stronger. A model becomes recommendation-eligible only after enough CKB validation observations exist for that exact project/task profile, with rollback-rate gating. Verified profiles must also be fresh and lifecycle-eligible. Automatic model selection and automatic execution remain disabled in V13 until those policies are separately validated.

Evaluation writes are idempotent when a caller supplies an evaluation ID and require at least one actually observed outcome. An empty/unobserved evaluation cannot be written merely to create a score row.

## Automatic learning from Guarded Change

CKB Cloud V13 includes an observed-only transaction learning worker. It consumes persisted Guarded Change transactions after post-change rescan/rollback states and extracts only validations that were actually recorded.

It can learn:

- compile/build/typecheck outcomes,
- test outcomes,
- contract/OpenAPI/schema compatibility outcomes,
- security/SAST/audit outcomes,
- explicit runtime-regression booleans,
- whether rollback was actually required.

The worker uses deterministic evaluation IDs per transaction, so repeated passes update the same observation instead of inflating sample size. Unattributed transactions are not assigned to a model. Missing checks remain `null`.

The worker does **not** run a model, change source, select a model, merge, push or deploy code.

## Guarded self-evolution

The intended loop is:

```text
OBSERVE
  ↓
LEARN
  ↓
PROPOSE
  ↓
SIMULATE
  ↓
VALIDATE
  ↓
EXPLICIT PROMOTION
  ↓
MONITOR / RESCAN / RETRACE
  ↓
ROLL BACK IF REQUIRED
```

V13 deliberately sets autonomous source promotion to **false**. CKB may automatically update architecture memory, runtime evidence, history, verified model metadata through the trusted data plane, and observed model scorecards. It may also propose its own improvements. It must not silently self-deploy production source changes.

## Existing CKB systems reused by V13

V13 is not a second architecture engine. It composes existing evidence systems:

- Tree-sitter multi-language source analysis,
- dependency/call/type graphs,
- bounded Architecture Memory retrieval,
- causal paths and failure cones,
- Code DNA heuristics,
- architecture snapshots and diffs / Time Machine,
- Git drift analysis,
- test-gap analysis,
- OTLP runtime telemetry,
- Guarded Change transactions,
- VS Code / JetBrains / MCP integrations.

## Architecture Query Language

The V13 query layer resolves user/model intent into deterministic engines rather than making an LLM rediscover graph operations. Canonical operations include memory retrieval, path/failure-cone reasoning, impact analysis, Code DNA, snapshot history/diff and runtime evidence. Natural language can still be mapped into these operations while CKB preserves and exposes the resolved evidence operation.

## Continuous learning boundaries

CKB architecture memory is allowed to self-update from verified inputs:

- completed source scans,
- incremental repository graph deltas,
- persisted snapshots,
- observed OTLP telemetry,
- executed validation results,
- explicit user decisions/feedback,
- trusted primary-source model capability/lifecycle metadata.

No unverified model statement is written into the canonical architecture graph as a fact. No provider capability or lifecycle record changes source/runtime evidence classification.

## Current V13 branch status

V13 is being developed on `agent/architecture-intelligence-v13` while V12 UI deployment is independently verified. The branch is intentionally not promoted to `main` until blocked CI can execute compiler/tests and the Cloud build can be validated.
