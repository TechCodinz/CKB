---
name: ckb-architectural-intelligence
description: Query CKB (Code Knowledge Base) for static dependency graphs, dynamic runtime execution telemetry, blast-radius impact analysis, architectural drift checks, prompt context slicing, and self-healing refactoring.
---

# CKB Architectural Intelligence & Dynamic Telemetry for Antigravity

Use this skill whenever analyzing codebase architecture, evaluating change impact, checking architectural drift rules, or monitoring live dynamic runtime telemetry.

## Available Capabilities & MCP Tools

1. **Impact & Blast Radius Analysis (`ckb_analyze_impact`)**:
   - Call before modifying existing files to evaluate transitive compile/runtime breaks.
2. **Dynamic Runtime Execution Telemetry (`ckb_get_dynamic_runtime_metrics` & `ckb_record_dynamic_telemetry`)**:
   - Ingests and inspects live runtime invocation frequency, execution latency, and hotpath status.
3. **Topological Prompt Context Slicing (`ckb_get_prompt_context`)**:
   - Extract minimal token-optimized XML graph context slice for target file.
4. **Pre-Flight Agentic Diff Guardrail (`ckb_agentic_diff_guardrail`)**:
   - Validate proposed imports and file changes against boundary rules before saving.
5. **Self-Healing Refactoring Engine (`ckb_self_healing_refactor`)**:
   - Compute graph-partitioning interface isolation plan for circular dependency cycles.
6. **Predictive Failure Probability Index (`ckb_predict_failure_risk`)**:
   - Calculate failure probability risk score based on degree centrality and runtime hotpath status.

## Standard Workflow for Antigravity Agent
1. Before altering core abstractions, query `ckb_analyze_impact`.
2. Before adding new imports, query `ckb_agentic_diff_guardrail`.
3. To inspect live execution hotpaths, query `ckb_get_dynamic_runtime_metrics`.
4. If circular dependencies are found, invoke `ckb_self_healing_refactor`.
