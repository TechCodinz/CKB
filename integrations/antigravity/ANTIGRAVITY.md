# CKB Integration
# Place this file at your project root as ANTIGRAVITY.md
# Antigravity reads this at the start of every session

## Architectural Intelligence Available

CKB MCP Server: http://localhost:3000

## Required Behaviors

**Before modifying any existing file:**
- Call `ckb_analyze_impact` with the target file path and line number
- Report the risk_score to the user
- If risk_score > 0.7: state "⚠️ High-risk change" and ask for confirmation

**Before adding any new feature:**
- Call `ckb_check_boundaries` to identify correct architectural layer
- Call `ckb_get_patterns` to follow existing code conventions
- Never create a new module if CKB shows an existing one that fits

**During refactoring sessions:**
- Track cumulative impact — call `ckb_analyze_impact` for each file changed
- Stop and report if total transitively_affected count exceeds 20 files

## Architecture Rules
- No circular dependencies between any modules
- Domain layer must NOT import from Application or Infrastructure
- Application layer must NOT import from Infrastructure directly (use interfaces)
- Stable components (fan-in > 10) must not gain new outgoing dependencies
