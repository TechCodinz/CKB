# CKB for VS Code

> Cursor-driven software reality, exact runtime evidence and architecture intelligence inside Visual Studio Code.

## What makes the extension different

CKB does not treat a repository as a flat list of files. The extension continuously relates the editor cursor to the larger software system and keeps three evidence classes separate:

- **STATIC** — scanned source/AST and architecture relationships
- **RUNTIME** — exact observed execution from telemetry
- **PREDICTED** — impact/ripple simulation before a change

Static dependencies are never animated or highlighted as if they executed.

## Invisible Reality V10

### Cursor-Driven Semantic Editor Reality

Move through source and CKB resolves the current software depth:

`LINE → CALL → SYMBOL → FILE → SUBSYSTEM → SYSTEM`

In **AUTO** mode the depth responds to source selection and the visible editor scale. You can also control it explicitly:

- `CKB: Semantic Zoom In`
- `CKB: Semantic Zoom Out`
- `CKB: Semantic Zoom Auto`
- `CKB: Inspect Semantic Reality at Cursor`

The editor status item exposes the current semantic target. Hovering the highlighted source shows system/subsystem/file/symbol context plus fan-in/fan-out and change-sensitivity evidence when the local deep architecture bundle resolves the cursor.

### Exact Runtime Source Fusion

When the configured Reality server provides `exact-observed-span-instances`, CKB maps matching trace identities back to the active file/symbol and highlights the observed source/target context directly in the editor.

Runtime context can include:

- HTTP
- database
- cache
- queue
- event
- WebSocket
- function/internal calls
- observed duration and error state

If telemetry is unavailable, the runtime layer stays off. CKB does not create decorative fake execution.

### Molecular + Live Reality Sidebar

The **CKB Invisible Reality** activity-bar view includes:

- deep architecture activity and hotspots
- semantic/molecular/state/nanotrace lenses
- exact runtime transmission filtering
- bounded architecture memory
- cursor change-ripple access
- Cloud Living Universe continuity

### IDE → Cloud Cursor Reality Continuity

The editor context menu can now carry the current navigation target into CKB Cloud without embedding source contents in the URL:

- `CKB: Continue Cursor Reality in Cloud` opens the same file/line in Cloud X-Ray.
- `CKB: Ask Raiziom About Cursor Reality` opens the same navigation context and requests the global evidence-grounded Raiziom console.

The continuity link can include the relative file path, line/column, current identifier and semantic depth hint. Those values are treated as navigation/advisory context; Cloud architecture and runtime evidence remain authoritative.

## Core features

- **🔬 Semantic Editor Reality** — Cursor-aware LINE/CALL/SYMBOL/FILE/SUBSYSTEM/SYSTEM navigation
- **🟢 Exact Runtime Evidence** — Runtime source context only when observed traces actually exist
- **☁️ Cursor Cloud Continuity** — Resume the same source target in Cloud X-Ray or Raiziom
- **🔍 Scan** — Full codebase architecture scan
- **⚠️ Inline Diagnostics** — Architecture findings in source files
- **⚡ Impact Analysis** — Predict direct/transitive change ripple at the cursor
- **🧠 Architecture Memory** — Bounded model-ready architecture context
- **🛡️ Architecture Check** — CI-style architecture guardrail check
- **🤖 MCP / Agent Integration** — Expose CKB architecture intelligence to compatible AI clients
- **🌌 Cloud Living Universe** — Continue investigations across deeper visual/runtime surfaces

## Requirements

For deep local architecture intelligence, install the current CKB CLI/intelligence binaries. Runtime source fusion additionally requires a CKB Reality server receiving telemetry for the project.

## Command Palette

| Command | Description |
|---------|-------------|
| `CKB: Inspect Semantic Reality at Cursor` | Inspect the current semantic source/system context |
| `CKB: Semantic Zoom In` | Descend toward call/line detail |
| `CKB: Semantic Zoom Out` | Ascend toward file/subsystem/system context |
| `CKB: Semantic Zoom Auto` | Let selection + visible editor scale resolve depth |
| `CKB: Continue Cursor Reality in Cloud` | Resume the current file/line in Cloud X-Ray |
| `CKB: Ask Raiziom About Cursor Reality` | Continue the cursor target in contextual Cloud Raiziom |
| `CKB: Open Invisible Reality` | Open the molecular/runtime architecture cockpit |
| `CKB: Deep Activity Analysis` | Rebuild activity, hotspots and architecture memory |
| `CKB: Query Architecture Memory` | Ask about a symbol, flow, responsibility or risk |
| `CKB: Analyze Change Ripple at Cursor` | Predict direct/transitive graph impact |
| `CKB: Scan Project` | Refresh the base architecture scan |
| `CKB: Check Architecture` | Run architecture guardrail checks |
| `CKB: Start MCP Server` | Start a local CKB server for compatible clients |

## Keyboard shortcuts

- **Inspect semantic reality:** `Shift+Alt+R`
- **Semantic zoom in:** `Shift+Alt+]`
- **Semantic zoom out:** `Shift+Alt+[` 
- **Cursor ripple:** `Shift+Ctrl+I` (`Shift+Cmd+I` on macOS)
- **Architecture memory:** `Shift+Ctrl+M` (`Shift+Cmd+M` on macOS)

## Example configuration

```json
{
  "ckb.serverUrl": "http://localhost:3000",
  "ckb.editorSemanticReality": true,
  "ckb.liveRuntimePolling": true,
  "ckb.runtimePollIntervalMs": 2500,
  "ckb.autoScanOnOpen": true,
  "ckb.showDiagnostics": true,
  "ckb.cloudExplorerUrl": "https://ckb-nu.vercel.app/project/current"
}
```

## Important evidence contract

CKB uses this rule throughout the extension:

- **STATIC** means the source/architecture graph says a relationship can exist.
- **RUNTIME** means telemetry observed execution.
- **PREDICTED** means a simulation or proposed change.

A missing runtime connection does not reduce static architecture usefulness; it simply means CKB refuses to pretend that an unobserved dependency executed.
