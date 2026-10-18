# CKB Integration — Claude Code
# This file is read automatically by Claude Code at session start

## Architectural Intelligence Available

This project has CKB (Code Knowledge Base) integrated via MCP.
You have access to real-time architectural intelligence.

## Required Behavior

**Before modifying any existing file:**
- Call `ckb_analyze_impact` and report the risk score to the user
- If risk_score > 0.7 — explicitly warn before proceeding

**Before adding any new feature:**
- Call `ckb_check_drift` to see current layer-boundary and drift violations before choosing a layer
- Call `ckb_generate_ai_rules` / `ckb_get_prompt_context` to follow established conventions

**Before creating any new file:**
- Check if CKB shows an existing module that should be extended instead

## Architecture Rules (enforced by CKB scan)
- No circular dependencies between modules
- Lower layers (Domain) must NOT import from higher layers (Infrastructure)
- Follow the conventions surfaced by `ckb_generate_ai_rules` and `ckb_get_prompt_context`
- "God objects" (>20 deps) must not gain new dependencies without approval

## Commands Available
- `ckb scan <path>` — full scan (run this if no report exists yet)
- `ckb check` — pass/fail architecture check
- `ckb impact <file> <line>` — impact analysis
- `ckb serve` — start MCP server
