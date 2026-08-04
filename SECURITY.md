# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| latest  | ✅ Yes    |

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

If you discover a security vulnerability in CKB (the engine, CLI, MCP server, IDE extensions, or SDKs), please report it by emailing:

**security@techcodinz.com**

Include:
- A description of the vulnerability and its potential impact
- Steps to reproduce (proof-of-concept if possible)
- The component affected (`core/`, `cli/`, `mcp-server/`, `integrations/vscode/`, etc.)

You will receive an acknowledgement within **48 hours** and a more detailed response within **5 business days** indicating the next steps.

## Scope

This repository covers the open-source components:
- `core/` — the analysis engine (Rust)
- `cli/` — the `ckb-cli` binary (Rust)
- `mcp-server/` — the REST/MCP server (Rust)
- `bindings/` — Node.js, Python, and WASM SDKs
- `integrations/vscode/` — VS Code extension
- `integrations/jetbrains/` — JetBrains plugin
- `.github/actions/ckb-scan/` — GitHub Action

The hosted cloud backend and billing infrastructure are **not** in this repository and are covered separately.

## Security Considerations

CKB reads source code from your filesystem and sends it to the configured MCP server endpoint. Before using CKB:

- **Self-hosted**: The MCP server can be run locally — your code never leaves your machine.
- **Hosted**: If using our hosted endpoint, review our [Privacy Policy](https://techcodinz.com/privacy) for details on data handling.
- **API Keys**: Always use environment variables for `CKB_API_KEY` — never commit keys to source control. The `.env.example` file shows all required variables; none contain real credentials.

## Disclosure Policy

We follow [Responsible Disclosure](https://en.wikipedia.org/wiki/Coordinated_vulnerability_disclosure). Once a fix is ready, we will:
1. Release a patched version
2. Credit the reporter (unless they prefer to remain anonymous)
3. Publish a summary in the release notes
