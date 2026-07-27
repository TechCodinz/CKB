# CKB Architectural Intelligence & Dynamic Telemetry for Antigravity IDE
# Antigravity reads this at the start of every session

## Architectural Intelligence Available
- CKB Stdio MCP Server: `ckb serve --stdio`
- CKB REST Server: `http://localhost:3000`

## Required Agent Behaviors

**Before modifying any existing file:**
- Call `ckb_analyze_impact` with the target file path and line number
- Call `ckb_predict_failure_risk` to calculate Predictive Failure Probability Score
- Report the `risk_score` to the user
- If `risk_score > 0.7`: state "⚠️ High-risk change" and ask for confirmation

**Before adding any new feature or refactoring:**
- Call `ckb_get_prompt_context` to retrieve minimal topological graph context
- Call `ckb_agentic_diff_guardrail` to verify layer boundaries before applying file modifications
- Query `ckb_get_dynamic_runtime_metrics` to inspect live execution hotpaths and latency

**During refactoring & debt remediation:**
- If circular dependencies or tight coupling are detected, invoke `ckb_self_healing_refactor` to compute graph-partitioning interface isolation plans
- Call `ckb_generate_ai_rules` to re-synthesize updated architectural guidelines

## Architecture Rules
- No circular dependencies between any modules (`core`, `mcp-server`, `cli`, `web`)
- Core Rust engine must NOT depend on network/HTTP servers
- Maintain zero-copy non-blocking asynchronous operations in `core` and `mcp-server`
- Preserve AST multi-language parsing fidelity for TypeScript, Python, Go, Rust, and Java
