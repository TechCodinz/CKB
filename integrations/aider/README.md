# CKB for Aider — Setup Guide

Aider is an AI pair-programming CLI. CKB gives Aider architectural context.

## Setup

### Step 1: Start CKB Server and Scan
```bash
ckb serve --cors &
ckb scan /your/project
```

### Step 2: Generate Architecture Context for Aider
```bash
# Export your architecture as a Markdown context file
ckb report /your/project --format markdown > .aider-ckb-context.md
```

### Step 3: Always Include CKB Context in Aider Sessions
```bash
# Option A: Pass as read-only context file
aider --read .aider-ckb-context.md --read ARCHITECTURE.md

# Option B: Add to .aider.conf.yml (recommended)
cat integrations/aider/.aider.conf.yml >> ~/.aider.conf.yml
```

### Step 4: Add Pre-Commit Hook for Architecture Checks
```bash
cp integrations/aider/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

## .aider.conf.yml Integration

```yaml
# CKB architectural context always loaded
read:
  - .aider-ckb-context.md
  - ARCHITECTURE.md

# Auto-refresh CKB context before each session
auto_commits: true
```

## Workflow

```bash
# 1. Before coding — refresh architecture context
ckb report . --format markdown > .aider-ckb-context.md

# 2. Start aider with CKB context
aider --read .aider-ckb-context.md src/auth/login.ts

# 3. Aider now knows:
#    - Your layer boundaries
#    - Current violations
#    - Stable vs unstable modules
#    - Impact of changes
```

## Pre-commit Hook

The `pre-commit` hook automatically runs `ckb check` before every commit.
If violations are found, it blocks the commit and shows them.

```bash
# Install
cp integrations/aider/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```
