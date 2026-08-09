//! Git History Architectural Drift Timeline
//!
//! V2 principle: history shown to users must come from real Git commits. This
//! module therefore fails explicitly when Git cannot be read and records real
//! per-commit file/churn data. Architectural-risk fields remain clearly named
//! estimates until commit-by-commit graph rescanning is enabled.

use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftTimelineEntry {
    pub commit_hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
    pub files_changed: Vec<String>,
    pub additions: usize,
    pub deletions: usize,
    pub estimated_violations_introduced: usize,
    pub risk_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftTimeline {
    pub commits_analyzed: usize,
    pub total_violations_over_time: usize,
    pub highest_risk_commit: Option<DriftTimelineEntry>,
    pub trend: DriftTrend,
    pub entries: Vec<DriftTimelineEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DriftTrend {
    Improving,
    Stable,
    Worsening,
}

#[derive(Default)]
struct CommitAccumulator {
    meta: Option<(String, String, String, String)>,
    files: Vec<String>,
    additions: usize,
    deletions: usize,
}

pub struct GitDriftAnalyzer;

impl GitDriftAnalyzer {
    pub fn build_timeline(repo_path: &str, max_commits: usize) -> anyhow::Result<DriftTimeline> {
        if max_commits == 0 {
            return Ok(DriftTimeline {
                commits_analyzed: 0,
                total_violations_over_time: 0,
                highest_risk_commit: None,
                trend: DriftTrend::Stable,
                entries: vec![],
            });
        }

        let limit = max_commits.min(500).to_string();
        let output = Command::new("git")
            .args([
                "-C", repo_path,
                "log",
                "--format=CKB_COMMIT|%H|%an|%aI|%s",
                "--numstat",
                "-n", &limit,
            ])
            .output()
            .map_err(|e| anyhow::anyhow!("failed to execute git for architecture history: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("git history unavailable for {repo_path}: {}", stderr.trim()));
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        let mut entries = Vec::new();
        let mut current = CommitAccumulator::default();

        for line in raw.lines() {
            if let Some(meta) = line.strip_prefix("CKB_COMMIT|") {
                Self::flush(&mut current, &mut entries);
                let parts: Vec<&str> = meta.splitn(4, '|').collect();
                if parts.len() == 4 {
                    current.meta = Some((
                        parts[0].to_string(),
                        parts[1].to_string(),
                        parts[2].to_string(),
                        parts[3].to_string(),
                    ));
                }
                continue;
            }

            if line.trim().is_empty() { continue; }
            let cols: Vec<&str> = line.splitn(3, '\t').collect();
            if cols.len() != 3 { continue; }

            // Binary changes are represented by '-' in numstat. We keep the
            // file in the real change set but do not invent line counts.
            if cols[0] != "-" { current.additions += cols[0].parse::<usize>().unwrap_or(0); }
            if cols[1] != "-" { current.deletions += cols[1].parse::<usize>().unwrap_or(0); }
            current.files.push(cols[2].to_string());
        }
        Self::flush(&mut current, &mut entries);

        let total_violations = entries.iter().map(|e| e.estimated_violations_introduced).sum();
        let highest_risk = entries.iter()
            .max_by(|a, b| a.risk_score.partial_cmp(&b.risk_score).unwrap_or(std::cmp::Ordering::Equal))
            .cloned();
        let trend = Self::compute_trend(&entries);

        Ok(DriftTimeline {
            commits_analyzed: entries.len(),
            total_violations_over_time: total_violations,
            highest_risk_commit: highest_risk,
            trend,
            entries,
        })
    }

    fn flush(current: &mut CommitAccumulator, entries: &mut Vec<DriftTimelineEntry>) {
        let Some((hash, author, date, message)) = current.meta.take() else { return; };
        current.files.sort();
        current.files.dedup();
        let estimated_violations = Self::estimate_boundary_risk(&current.files);
        let risk = Self::compute_risk(
            current.files.len(),
            current.additions,
            current.deletions,
            estimated_violations,
        );
        entries.push(DriftTimelineEntry {
            commit_hash: hash,
            author,
            date,
            message,
            files_changed: std::mem::take(&mut current.files),
            additions: std::mem::take(&mut current.additions),
            deletions: std::mem::take(&mut current.deletions),
            estimated_violations_introduced: estimated_violations,
            risk_score: risk,
        });
    }

    /// This is deliberately an estimate, not a detected violation. It flags
    /// commits touching multiple architectural zones so callers know which
    /// historical commits deserve a true graph-rescan comparison.
    fn estimate_boundary_risk(files: &[String]) -> usize {
        let boundary_keywords = [
            "core", "api", "ui", "web", "infra", "domain", "service",
            "controller", "database", "storage", "auth", "billing",
        ];
        let layers: std::collections::HashSet<&str> = files.iter()
            .flat_map(|f| boundary_keywords.iter().filter(|k| f.to_ascii_lowercase().contains(**k)).copied())
            .collect();
        layers.len().saturating_sub(1)
    }

    fn compute_risk(files: usize, additions: usize, deletions: usize, boundary_crossings: usize) -> f32 {
        let churn = additions.saturating_add(deletions) as f32;
        let churn_factor = (churn.ln_1p() / 10.0).min(0.35);
        let file_factor = (files as f32 / 40.0).min(0.25);
        let boundary_factor = (boundary_crossings as f32 * 0.12).min(0.40);
        (churn_factor + file_factor + boundary_factor).min(1.0)
    }

    fn compute_trend(entries: &[DriftTimelineEntry]) -> DriftTrend {
        if entries.len() < 6 { return DriftTrend::Stable; }
        let window = entries.len().min(5);
        let recent = entries.iter().take(window).map(|e| e.risk_score).sum::<f32>() / window as f32;
        let older = entries.iter().rev().take(window).map(|e| e.risk_score).sum::<f32>() / window as f32;
        if recent > older + 0.08 { DriftTrend::Worsening }
        else if recent < older - 0.08 { DriftTrend::Improving }
        else { DriftTrend::Stable }
    }
}
