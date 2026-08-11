---
name: ckb-change-impact
description: Analyze the blast radius, causal path, downstream failure cone, snapshots, and architecture guardrails for a proposed code change using CKB when the user asks what could break, what depends on a component, or how to change code more safely.
---

Use this skill when the user's main goal is change-risk analysis rather than a general repository overview.

1. Identify the repository/project plus the exact file/component and change the user is considering. If the project has not been scanned in CKB, call `ckb_scan_repository` before impact analysis.
2. Call `ckb_analyze_impact` with the repository-relative file, relevant line when known, and the best matching `change_type`. Do not invent a line number when the user supplies one; preserve it exactly.
3. Use the returned CKB graph evidence to identify directly affected and transitive areas. Keep CKB-computed impact separate from your own engineering interpretation.
4. If the user provides or the graph reveals specific source and target node IDs whose relationship matters, call `ckb_find_causal_path` to explain how the effect propagates.
5. When the user wants downstream failure exposure for a component, call `ckb_get_failure_cone` with that CKB node as the root.
6. Call `ckb_get_test_gaps` when impact analysis needs a validation plan. Prioritize tests around affected high-centrality or insufficiently covered paths rather than giving a generic test checklist.
7. When snapshots exist and the user asks what changed between architecture states, call `ckb_list_snapshots` and then `ckb_diff_snapshots` for the selected pair.
8. Call `ckb_generate_ai_rules` when the user wants architecture-aware coding guardrails for implementing the change. Clearly state that the returned rules are recommendations and are not written into the repository by this read-only CKB integration.
9. Finish with a compact change-safety brief: risk level supported by CKB evidence, affected areas, causal/failure paths, test priorities, and recommended implementation order.
10. Never claim that CKB verified behavior it did not observe. Static graph evidence, Git history, runtime telemetry, and inferred engineering advice must remain distinguishable.
