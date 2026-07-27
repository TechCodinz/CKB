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
- Call `ckb_check_boundaries` to determine the correct layer
- Call `ckb_get_patterns` to follow established conventions

**Before creating any new file:**
- Check if CKB shows an existing module that should be extended instead

## Architecture Rules (enforced by CKB scan)
- No circular dependencies between modules
- Lower layers (Domain) must NOT import from higher layers (Infrastructure)
- Follow the patterns detected by `ckb_get_patterns`
- "God objects" (>20 deps) must not gain new dependencies without approval

## Commands Available
- `ckb scan <path>` — full scan (run this if no report exists yet)
- `ckb check` — pass/fail architecture check
- `ckb impact <file> <line>` — impact analysis
- `ckb serve` — start MCP server
