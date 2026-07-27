---
name: ckb-architectural-intelligence
description: Query CKB (Code Knowledge Base) for static dependency graphs, dynamic OTLP telemetry, semantic clone detection, git history drift timelines, cross-service API contract validation, test coverage gap analysis, and multi-repo federation.
---

# CKB Architectural Intelligence & Dynamic Telemetry for Antigravity

Use this skill whenever analyzing codebase architecture, evaluating change impact, checking drift rules, monitoring live telemetry, or validating cross-repo contracts.

## Available Capabilities & MCP Tools

1. **Impact & Blast Radius Analysis (`ckb_analyze_impact`)**: Call before modifying existing files.
2. **OpenTelemetry OTLP Ingestion (`ckb_ingest_otlp_spans`)**: Ingests OTLP JSON spans to populate hotpaths automatically.
3. **Semantic Clone Detector (`ckb_detect_semantic_clones`)**: Finds duplicate logic via normalized AST rolling hash fingerprinting.
4. **Git Drift Timeline (`ckb_get_drift_timeline`)**: Tracks architectural violations and risk scores across Git commits.
5. **Cross-Service API Contract Validator (`ckb_validate_api_contracts`)**: Validates OpenAPI specs between services before deploy.
6. **AI Test Coverage Gap Analyzer (`ckb_analyze_test_coverage_gaps`)**: Maps test call graph to production hotpaths.
7. **Multi-Repo Federation Engine (`ckb_federate_repos`)**: Merges graphs across multiple repos into a unified view.
8. **Topological Prompt Context Slicing (`ckb_get_prompt_context`)**: Extracts minimal token-optimized XML graph context.
9. **Pre-Flight Agentic Diff Guardrail (`ckb_agentic_diff_guardrail`)**: Validates proposed code diffs against boundary rules.
10. **Self-Healing Refactoring Engine (`ckb_self_healing_refactor`)**: Generates interface isolation plans for circular cycles.
11. **Predictive Failure Probability Index (`ckb_predict_failure_risk`)**: Calculates failure risk score based on degree centrality.

## Standard Workflow for Antigravity Agent
1. Before altering core abstractions, query `ckb_analyze_impact`.
2. Before adding new imports, query `ckb_agentic_diff_guardrail`.
3. To inspect live execution hotpaths, query `ckb_get_dynamic_runtime_metrics` or `ckb_ingest_otlp_spans`.
4. To check cross-service safety before deploy, query `ckb_validate_api_contracts`.
5. If circular dependencies are found, invoke `ckb_self_healing_refactor`.
