# CKB Universal Model Gateway

CKB's model integration layer is provider-neutral. The canonical architecture-intelligence contract is the remote MCP endpoint:

```text
https://ckb-mcp-server.onrender.com/mcp
```

The same production edge also exposes a normalized function-tool adapter for models or agent runtimes that do not consume remote MCP directly:

```text
GET  /llm/capabilities
GET  /llm/tools?provider=<provider>
POST /llm/call
```

All paths reuse the same 13 CKB tools, OAuth/API-key authorization boundary, per-user architecture namespace, Reality engine, graph persistence, runtime evidence, architecture memory, and repository-read-only policy.

## Native remote MCP: preferred path

Use `/mcp` whenever the model/client supports Streamable HTTP MCP. As of August 2026, current official provider documentation includes remote MCP support for OpenAI clients, Claude/Claude Code ecosystems, xAI Grok, and Google Gemini Interactions. Other standards-compatible MCP clients can connect to the same endpoint.

Native MCP is preferred because the client receives the original MCP tool metadata, security schemes, annotations, structured output, and OAuth account-linking challenge without translation.

### Claude Code

```bash
claude mcp add --transport http ckb https://ckb-mcp-server.onrender.com/mcp
```

### Grok

```bash
grok mcp add --transport http ckb https://ckb-mcp-server.onrender.com/mcp
```

### Gemini Interactions

Configure a remote MCP tool using the CKB URL as the Streamable HTTP server:

```text
name: ckb
url: https://ckb-mcp-server.onrender.com/mcp
```

### ChatGPT / Codex

Register the same `/mcp` endpoint through the supported custom MCP/plugin flow. CKB publishes OAuth protected-resource discovery for account linking.

## Function-tool adapter

For runtimes that use JSON function calling rather than MCP, fetch the provider-shaped CKB tool definitions and execute selected calls through the universal bridge.

### Supported schema views

```text
/llm/tools?provider=openai
/llm/tools?provider=deepseek
/llm/tools?provider=xai
/llm/tools?provider=anthropic
/llm/tools?provider=gemini
/llm/tools?provider=generic
/llm/tools?provider=mcp
```

`openai`, `deepseek`, and `xai` return OpenAI-compatible function-tool envelopes. `anthropic` returns Anthropic-style `input_schema` tools. `gemini` returns Gemini Interactions-style function tools. `generic` returns provider-neutral JSON Schema definitions. `mcp` returns the original MCP tool list unchanged.

### DeepSeek example flow

1. Fetch CKB tools:

```bash
curl https://ckb-mcp-server.onrender.com/llm/tools?provider=deepseek
```

2. Send those function definitions to DeepSeek in the model request.
3. When DeepSeek produces a CKB tool call, forward the selected name and parsed arguments to CKB:

```bash
curl -X POST https://ckb-mcp-server.onrender.com/llm/call \
  -H "Authorization: Bearer <CKB OAuth access token or operator credential>" \
  -H "Content-Type: application/json" \
  -d '{
    "provider": "deepseek",
    "name": "ckb_get_architecture_graph",
    "arguments": {"project_id": "TechCodinz_CKB"}
  }'
```

4. Feed the returned `result` to the model as its tool result.

## Project identifiers

Use stable logical project IDs containing only letters, digits, `_`, and `-`, for example:

```text
TechCodinz_CKB
ada_scanner_pro
payments-service-v2
```

Do not use filesystem paths, URLs, spaces, colons, or arbitrary punctuation as `project_id`. Repository URLs belong only in `github_url` during `ckb_scan_repository`.

OAuth callers are automatically isolated into a per-user CKB namespace before Reality persistence, so two users can use the same logical project ID without sharing project state.

## Authentication

The universal function bridge does not introduce a weaker authentication path. `/llm/call` invokes the canonical MCP tool-call handler internally.

- OAuth access tokens are validated through CKB Cloud introspection.
- Tool scopes remain `architecture:read` and `repository:scan`.
- Trusted infrastructure can use the existing operator/API-key boundary.
- Missing OAuth credentials produce HTTP 401 with `WWW-Authenticate` and the CKB protected-resource metadata location.
- Tool discovery and capability discovery remain public.

## Architecture

```text
                         CKB UNIVERSAL MODEL GATEWAY
                                      |
                    +-----------------+-----------------+
                    |                                   |
             Native Streamable MCP                Function adapters
                    |                                   |
       ChatGPT / Claude / Grok / Gemini      DeepSeek / local / custom LLMs
                    |                                   |
                    +-----------------+-----------------+
                                      |
                              canonical MCP handler
                                      |
                     OAuth + scopes + tenant isolation
                                      |
                              Reality gateway
                                      |
                              Reality v5 engine
                                      |
                     CKB Core / graph / memory / OTLP
```

The important invariant is that provider adapters never implement architecture intelligence themselves. They only translate tool definitions and calls. CKB Reality remains the single source of architectural evidence.

## Current provider references

- xAI Remote MCP: https://docs.x.ai/developers/tools/remote-mcp
- xAI MCP servers: https://docs.x.ai/build/features/mcp-servers
- DeepSeek tool calls: https://api-docs.deepseek.com/guides/tool_calls
- Google Gemini function calling / remote MCP: https://ai.google.dev/gemini-api/docs/function-calling

Provider APIs change over time. Prefer native MCP whenever available and keep adapter schema mappings covered by smoke/integration tests.
