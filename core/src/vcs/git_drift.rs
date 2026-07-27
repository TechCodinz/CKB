//! Git History Architectural Drift Timeline
//! Parses git log output to correlate commits with architectural violations over time

use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftTimelineEntry {
    pub commit_hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
    pub files_changed: Vec<String>,
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

pub struct GitDriftAnalyzer;

impl GitDriftAnalyzer {
    /// Analyze git history in `repo_path` and build a drift timeline
    pub fn build_timeline(repo_path: &str, max_commits: usize) -> anyhow::Result<DriftTimeline> {
        let output = Command::new("git")
            .args([
                "-C", repo_path,
                "log",
                "--format=%H|%an|%ad|%s",
                "--date=short",
                "--name-only",
                &format!("-{}", max_commits),
            ])
            .output();

        let raw = match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => {
                // Return a synthetic placeholder if git is unavailable in this environment
                return Ok(DriftTimeline {
                    commits_analyzed: 0,
                    total_violations_over_time: 0,
                    highest_risk_commit: None,
                    trend: DriftTrend::Stable,
                    entries: vec![],
                });
            }
        };

        let mut entries: Vec<DriftTimelineEntry> = Vec::new();
        let mut current_meta: Option<(String, String, String, String)> = None;
        let mut current_files: Vec<String> = Vec::new();

        for line in raw.lines() {
            if line.contains('|') && line.split('|').count() == 4 {
                // Flush previous commit
                if let Some((hash, author, date, msg)) = current_meta.take() {
                    let violations = Self::estimate_violations(&current_files);
                    let risk = Self::compute_risk(&current_files, violations);
                    entries.push(DriftTimelineEntry {
                        commit_hash: hash,
                        author,
                        date,
                        message: msg,
                        files_changed: current_files.clone(),
                        estimated_violations_introduced: violations,
                        risk_score: risk,
                    });
                    current_files.clear();
                }

                let parts: Vec<&str> = line.splitn(4, '|').collect();
                current_meta = Some((
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts[2].to_string(),
                    parts[3].to_string(),
                ));
            } else if !line.trim().is_empty() {
                current_files.push(line.trim().to_string());
            }
        }

        // Flush last commit
        if let Some((hash, author, date, msg)) = current_meta {
            let violations = Self::estimate_violations(&current_files);
            let risk = Self::compute_risk(&current_files, violations);
            entries.push(DriftTimelineEntry {
                commit_hash: hash,
                author,
                date,
                message: msg,
                files_changed: current_files,
                estimated_violations_introduced: violations,
                risk_score: risk,
            });
        }

        let total_violations: usize = entries.iter().map(|e| e.estimated_violations_introduced).sum();
        let highest_risk = entries.iter().max_by(|a, b| a.risk_score.partial_cmp(&b.risk_score).unwrap()).cloned();

        let trend = Self::compute_trend(&entries);

        Ok(DriftTimeline {
            commits_analyzed: entries.len(),
            total_violations_over_time: total_violations,
            highest_risk_commit: highest_risk,
            trend,
            entries,
        })
    }

    /// Heuristic: cross-layer files in same commit = likely boundary violation
    fn estimate_violations(files: &[String]) -> usize {
        let boundary_keywords = ["core", "api", "ui", "infra", "domain", "service", "controller"];
        let layers: std::collections::HashSet<&str> = files.iter()
            .flat_map(|f| boundary_keywords.iter().filter(|k| f.contains(*k)).copied())
            .collect();
        if layers.len() > 1 { layers.len() - 1 } else { 0 }
    }

    fn compute_risk(files: &[String], violations: usize) -> f32 {
        let base = violations as f32 * 0.3;
        let file_factor = (files.len() as f32 / 20.0).min(0.5);
        (base + file_factor).min(0.99)
    }

    fn compute_trend(entries: &[DriftTimelineEntry]) -> DriftTrend {
        if entries.len() < 3 {
            return DriftTrend::Stable;
        }
        let recent: f32 = entries.iter().take(3).map(|e| e.risk_score).sum::<f32>() / 3.0;
        let older: f32 = entries.iter().rev().take(3).map(|e| e.risk_score).sum::<f32>() / 3.0;
        if recent > older + 0.1 { DriftTrend::Worsening }
        else if recent < older - 0.1 { DriftTrend::Improving }
        else { DriftTrend::Stable }
    }
}
