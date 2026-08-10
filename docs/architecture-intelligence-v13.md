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

## Model capability profile

V13 accepts a neutral profile:

```json
{
  "provider": "optional-provider-name",
  "model": "optional-model-name",
  "contextWindowTokens": 200000,
  "supportsStructuredOutput": true,
  "supportsToolUse": true,
  "supportsParallelTools": false,
  "supportsImages": false,
  "supportsCodeExecution": false
}
```

These are declared capabilities, not CKB quality claims. Quality is learned only from observed evaluation outcomes.

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

V13 deliberately sets autonomous source promotion to **false**. CKB may automatically update architecture memory, runtime evidence, history and model scorecards. It may also propose its own improvements. It must not silently self-deploy production source changes.

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

## Architecture Query Language direction

The query layer should resolve user/model intent into existing deterministic engines rather than make an LLM rediscover graph operations. Planned canonical operations are:

```text
MEMORY <query> [DEPTH n] [LIMIT n]
PATH <source-symbol-id> -> <target-symbol-id> [DEPTH n]
DEPENDENTS <symbol-id> [DEPTH n]
IMPACT <source-path>[:line]
DNA [symbol-id]
HISTORY [snapshot-id]
DIFF <from-snapshot> -> <to-snapshot>
RUNTIME [symbol-id|trace-id]
```

Natural language remains supported; the orchestrator maps it into these evidence operations and can show the resolved operation to the caller.

## Continuous learning boundaries

CKB architecture memory is allowed to self-update from verified inputs:

- completed source scans,
- incremental repository changes once an exact graph-delta engine is active,
- persisted snapshots,
- observed OTLP telemetry,
- executed validation results,
- explicit user decisions/feedback.

No unverified model statement is written into the canonical architecture graph as a fact.

## Current V13 branch status

V13 is being developed on `agent/architecture-intelligence-v13` while V12 UI deployment is independently verified. The branch is intentionally not promoted to `main` until blocked CI can execute compiler/tests and the Cloud build can be validated.
