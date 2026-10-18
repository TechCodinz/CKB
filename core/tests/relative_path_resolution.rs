//! Regression: file-path accessors must resolve the natural
//! workspace-relative path ("src/core/util.ts") to the scanned File node,
//! whose stored ID embeds the path exactly as scanned (absolute here,
//! because the scan root is a tempdir). Before the fix,
//! `predict_failure_risk` built `"{file}::file"` verbatim and silently
//! scored 0.0 for relative paths, and the runtime-telemetry accessors
//! recorded/read an orphan ID no real node has, so telemetry recorded
//! under one path form was invisible to the other.
//!
//! The project below mirrors the functional fixture-a shape in miniature:
//! a shared utility imported by two files (fan-in 2).

use ckb_core::CkbEngine;
use std::fs;
use std::path::Path;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

#[tokio::test]
async fn relative_paths_resolve_for_prediction_and_telemetry() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("src/core/util.ts"),
        "export function formatId(prefix: string, n: number): string {\n  return prefix + \"-\" + n.toString();\n}\n",
    );
    write(
        &root.join("src/service/api.ts"),
        "import { formatId } from \"../core/util\";\n\nexport function handleRequest(): string {\n  return formatId(\"req\", 1);\n}\n",
    );
    write(
        &root.join("src/ui/widget.ts"),
        "import { formatId } from \"../core/util\";\n\nexport function renderWidget(): string {\n  return formatId(\"w\", 2);\n}\n",
    );

    let root_str = root.to_string_lossy().replace('\\', "/");
    let util_rel = "src/core/util.ts";
    let util_abs = format!("{}/{}", root_str, util_rel);

    let engine = CkbEngine::new().unwrap();
    engine.scan_codebase(&root_str).await.unwrap();

    // Failure prediction: relative == absolute, and non-zero (fan-in 2
    // gives a structural component of at least (2*2)/30).
    let p_rel = engine.predict_failure_risk(util_rel).await.unwrap();
    let p_abs = engine.predict_failure_risk(&util_abs).await.unwrap();
    assert!(p_abs > 0.1, "absolute-path score should be structural, got {}", p_abs);
    assert!(
        (p_rel - p_abs).abs() < 1e-6,
        "relative-path score {} must equal absolute-path score {}",
        p_rel, p_abs
    );

    // Telemetry recorded under the absolute (scanner-shaped) path must be
    // readable through the relative path.
    engine.record_runtime_telemetry(&util_abs, 2500, 120.0).await.unwrap();
    let metrics = engine
        .get_runtime_telemetry(util_rel)
        .await
        .unwrap()
        .expect("telemetry recorded under the absolute ID must resolve from the relative path");
    assert_eq!(metrics.execution_count, 2500);

    // And the reverse direction: recorded via relative, read via absolute.
    // Telemetry merges by summing execution counts, so 2500 + 3000 = 5500
    // also proves both writes landed on the same (real) node entry.
    engine.record_runtime_telemetry(util_rel, 3000, 130.0).await.unwrap();
    let metrics = engine
        .get_runtime_telemetry(&util_abs)
        .await
        .unwrap()
        .expect("telemetry recorded via the relative path must land on the scanned node");
    assert_eq!(metrics.execution_count, 5500);

    // The runtime component (2500+ executions => hotpath) must feed the
    // relative-path prediction too, raising it above the structural score.
    let p_rel_hot = engine.predict_failure_risk(util_rel).await.unwrap();
    assert!(
        p_rel_hot > p_rel,
        "runtime component missing for relative path: {} !> {}",
        p_rel_hot, p_rel
    );

    // A file that is not in the graph keeps the historical behaviour:
    // zero risk, no telemetry.
    assert_eq!(engine.predict_failure_risk("src/core/missing.ts").await.unwrap(), 0.0);
    assert!(engine.get_runtime_telemetry("src/core/missing.ts").await.unwrap().is_none());
}
