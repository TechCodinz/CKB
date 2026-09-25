use anyhow::{anyhow, Context, Result};
use ckb_core::{CkbEngine, Severity};
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Serialize)]
struct ArchitectureEvidence {
    score: f64,
    passed: bool,
    critical: usize,
    errors: usize,
    warnings: usize,
    info: usize,
}

fn architecture_evidence(violations: &[ckb_core::DriftViolation]) -> ArchitectureEvidence {
    let mut critical = 0usize;
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut info = 0usize;
    for violation in violations {
        match violation.severity {
            Severity::Critical => critical += 1,
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
            Severity::Info => info += 1,
        }
    }
    let penalty = (critical as f64 * 35.0)
        + (errors as f64 * 15.0)
        + (warnings as f64 * 4.0)
        + (info as f64);
    let score = (1.0 - (penalty.min(100.0) / 100.0)).clamp(0.0, 1.0);
    let passed = critical == 0 && score >= 0.80;
    ArchitectureEvidence { score, passed, critical, errors, warnings, info }
}

fn arg_value(name: &str) -> Option<String> {
    let mut args = std::env::args();
    while let Some(arg) = args.next() {
        if arg == name { return args.next(); }
    }
    None
}

#[tokio::main]
async fn main() -> Result<()> {
    let path = arg_value("--path").unwrap_or_else(|| ".".to_string());
    let build_id = arg_value("--build-id").ok_or_else(|| anyhow!("--build-id is required"))?;
    let project_id = arg_value("--project-id");
    let endpoint = std::env::var("OMNICODE_FEEDBACK_URL")
        .unwrap_or_else(|_| "https://omnicode-pro.vercel.app/api/integrations/learning-feedback".to_string());
    let secret = std::env::var("OMNICODE_CKB_FEEDBACK_SECRET")
        .context("OMNICODE_CKB_FEEDBACK_SECRET is required")?;

    let engine = CkbEngine::new()?;
    let report = engine.scan_codebase(&path).await
        .with_context(|| format!("CKB architecture scan failed for {}", path))?;
    let evidence = architecture_evidence(&report.drift);
    let event_key = format!("ckb:{}:architecture:v1", build_id);
    let mut body = json!({
        "eventKey": event_key,
        "buildId": build_id,
        "gate": "architecture",
        "passed": evidence.passed,
        "score": evidence.score,
        "source": "ckb"
    });
    if let Some(project_id) = project_id { body["projectId"] = json!(project_id); }

    let response = reqwest::Client::new()
        .post(endpoint)
        .header("x-omnicode-validation-secret", secret)
        .json(&body)
        .send().await
        .context("failed to reach OmniCode validation feedback endpoint")?;
    if !response.status().is_success() {
        let status = response.status();
        let message = response.text().await.unwrap_or_default();
        return Err(anyhow!("OmniCode feedback rejected CKB evidence: {} {}", status, message));
    }
    println!("{}", serde_json::to_string(&json!({"status":"ok","build_id":body["buildId"],"architecture":evidence}))?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckb_core::{DriftViolation, ViolationKind};
    use uuid::Uuid;

    fn violation(severity: Severity) -> DriftViolation {
        DriftViolation {
            id: Uuid::new_v4(),
            kind: ViolationKind::BoundaryCrossing,
            from: "a".to_string(),
            to: "b".to_string(),
            boundary: "test".to_string(),
            message: "test".to_string(),
            severity,
            suggested_fix: None,
        }
    }
    #[test]
    fn clean_report_passes() {
        let evidence = architecture_evidence(&[]);
        assert!(evidence.passed);
        assert_eq!(evidence.score, 1.0);
    }
    #[test]
    fn critical_breach_always_fails() {
        let evidence = architecture_evidence(&[violation(Severity::Critical)]);
        assert!(!evidence.passed);
        assert!(evidence.score < 0.80);
    }
    #[test]
    fn warnings_degrade_gradually() {
        let violations = vec![violation(Severity::Warning), violation(Severity::Warning)];
        let evidence = architecture_evidence(&violations);
        assert!(evidence.passed);
        assert!(evidence.score > 0.90);
    }
}
