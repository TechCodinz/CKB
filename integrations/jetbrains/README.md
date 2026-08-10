# CKB for JetBrains IDEs

CKB turns IntelliJ-platform editors into evidence-backed software-reality surfaces rather than flat source viewers.

## Semantic Editor Reality V10

The **CKB Semantic Editor V10** tool window follows the active text editor and resolves the current semantic depth:

`LINE → CALL → SYMBOL → FILE → SUBSYSTEM → SYSTEM`

AUTO mode uses the current selection and visible editor scale to choose the depth. Manual controls let the developer hold a particular layer while moving through source.

The view combines:

- PSI-resolved source symbol identity
- local CKB activity/hotspot evidence
- fan-in / fan-out
- activity and change sensitivity
- deterministic subsystem grouping
- exact observed runtime hop context when replay-safe telemetry maps to the current file/symbol

## Live Transmission Reality

The separate **CKB Live Transmission V8** tool window visualizes exact observed parent/child execution when telemetry is attached. Supported semantic flow classes include HTTP, database, cache, queue, event, WebSocket and function/internal calls.

Runtime animation is never generated from a static dependency edge.

## Molecular Microscope

The **CKB Invisible Reality** tool window provides semantic, molecular, nanotrace and state lenses over local architecture activity and memory.

## Truth contract

CKB keeps evidence classes separate:

- **STATIC** — source/AST and architecture graph
- **RUNTIME** — observed telemetry
- **PREDICTED** — impact/change simulation

The plugin does not synthesize missing runtime execution or claim a proposed change has mutated source before corresponding evidence exists.
