---
name: ckb-architecture-review
description: Review a software repository with CKB when the user wants to understand its architecture, important components, dependency structure, architecture memory, test gaps, runtime evidence, or architectural drift.
---

Use this skill for repository-level architecture review with the CKB MCP tools.

1. Establish the repository and a stable logical `project_id`. If the repository has not been scanned in the current CKB project namespace, call `ckb_scan_repository` first. Never claim a scan exists unless a CKB tool confirms it.
2. Call `ckb_get_architecture_graph` and use its nodes, edges, evidence, and metadata as the primary architectural source of truth.
3. Call `ckb_get_test_gaps` when the user asks about release readiness, weakly tested areas, regression risk, or engineering priorities.
4. Call `ckb_get_drift_history` when the user asks how architecture changed over time, whether boundaries are eroding, or which areas are becoming riskier.
5. Call `ckb_get_runtime_intelligence` when runtime/production behavior matters. Distinguish observed runtime evidence from static graph evidence; do not present absent telemetry as observed behavior.
6. Use `ckb_query_architecture_memory` for focused questions about a subsystem, file, component, symbol, or architectural concept. Use `ckb_get_code_dna` when a compact project-wide architecture-memory summary is useful.
7. When summarizing, separate CKB evidence from your interpretation. Highlight central components, high-risk coupling, test gaps, runtime hot paths when present, and concrete follow-up actions.
8. Do not invent nodes, edges, violations, telemetry, history, scores, or repository contents. If CKB returns incomplete evidence, say what is missing.
9. The exposed CKB workflow is read-only with respect to the target repository. Do not imply that a generated guardrail or recommendation was written back to source control.
