# Agents Configuration
# Codex CLI reads this file automatically

## CKB Architectural Intelligence Agent

This project has CKB (Code Knowledge Base) integrated.

### Required Agent Behaviors

Before modifying any file:
- Call `ckb_analyze_impact` on the target file
- Report risk_score to user if > 0.5
- If risk_score > 0.7: request explicit confirmation

Before adding new features:
- Call `ckb_check_boundaries` to determine correct architectural layer
- Call `ckb_get_patterns` to follow existing conventions

Architecture constraints enforced by CKB:
- No circular dependencies
- Strict layer separation (domain → application → infrastructure)
- Stable components (high fan-in) must not gain new outgoing dependencies
