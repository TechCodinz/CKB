# CKB Architecture Check — GitHub Action

Scans your codebase with CKB on every PR, posts (and keeps updated) a single
summary comment on the pull request, and optionally fails the build when
violations meet or exceed a configured severity.

## Usage in another repository

```yaml
# .github/workflows/ckb.yml
name: CKB Architecture Check
on:
  pull_request:
    branches: [main]

permissions:
  contents: read
  pull-requests: write   # required to post/update the PR comment

jobs:
  ckb-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: TechCodinz/CKB/.github/actions/ckb-scan@main
        with:
          path: '.'
          fail-on: 'error'   # info | warning | error | critical
          strict: 'true'     # 'false' to report only, never fail the build
```

## What it does

1. Builds the `ckb-cli` binary from source (cached between runs).
2. Runs `ckb-cli check <path> --report-format json --fail-on <severity>`.
3. Posts a single PR comment summarizing what was scanned and every
   violation found, grouped by severity — and **updates that same comment**
   on subsequent pushes instead of spamming the PR with a new one each time.
4. Fails the workflow (non-zero exit) if `strict: true` and any violation
   meets or exceeds `fail-on` severity.

## Inputs

| Input | Default | Description |
|---|---|---|
| `path` | `.` | Path to scan, relative to repo root. |
| `fail-on` | `error` | Minimum severity that fails the build: `info`, `warning`, `error`, `critical`. |
| `strict` | `true` | Set `false` to only report, never fail the workflow. |
| `comment-on-pr` | `true` | Set `false` to skip posting a PR comment. |
| `github-token` | `${{ github.token }}` | Token used to post the comment. Needs `pull-requests: write`. |

## Outputs

| Output | Description |
|---|---|
| `violations-found` | Total violation count from the scan. |
| `report-path` | Path to the raw `ckb-report.json` this run produced. |

## Known limitation

This currently builds `ckb-cli` from source on every run (a few minutes,
mitigated by `Swatinem/rust-cache`). If CKB ever publishes versioned binary
releases, swap the "Build ckb CLI" step in `action.yml` for a direct binary
download — that's a fast follow-up, not a redesign.
