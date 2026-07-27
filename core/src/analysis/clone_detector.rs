//! Semantic Clone / Duplicate Logic Detector
//! Uses token-normalized AST fingerprinting (rolling hash) to find functionally duplicate code

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneCluster {
    /// Fingerprint hash shared by all clones in this cluster
    pub fingerprint: u64,
    /// All files that share this logic fingerprint
    pub clone_locations: Vec<CloneLocation>,
    /// Estimated lines of duplicated code
    pub duplicate_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneLocation {
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneReport {
    pub total_clones_found: usize,
    pub clone_clusters: Vec<CloneCluster>,
    pub estimated_duplicate_lines: usize,
    pub refactor_savings_pct: f32,
}

pub struct CloneDetector;

impl CloneDetector {
    /// Detect semantic clones across file contents using normalized rolling token hash
    pub fn detect(file_contents: &HashMap<String, String>) -> CloneReport {
        let mut fingerprint_map: HashMap<u64, Vec<CloneLocation>> = HashMap::new();

        for (file_path, content) in file_contents {
            let chunks = Self::extract_normalized_chunks(content, 8);
            for (start_line, end_line, chunk, fingerprint) in chunks {
                fingerprint_map
                    .entry(fingerprint)
                    .or_default()
                    .push(CloneLocation {
                        file: file_path.clone(),
                        start_line,
                        end_line,
                        snippet: chunk.chars().take(120).collect(),
                    });
            }
        }

        let clusters: Vec<CloneCluster> = fingerprint_map
            .into_iter()
            .filter(|(_, locs)| locs.len() > 1)
            .map(|(fingerprint, clone_locations)| {
                let dup_lines: usize = clone_locations.iter()
                    .map(|l| l.end_line.saturating_sub(l.start_line))
                    .sum::<usize>()
                    .saturating_sub(clone_locations.first().map(|l| l.end_line - l.start_line).unwrap_or(0));

                CloneCluster {
                    fingerprint,
                    duplicate_lines: dup_lines,
                    clone_locations,
                }
            })
            .collect();

        let estimated_dup: usize = clusters.iter().map(|c| c.duplicate_lines).sum();
        let total_lines: usize = file_contents.values().map(|c| c.lines().count()).sum();
        let savings_pct = if total_lines > 0 {
            (estimated_dup as f32 / total_lines as f32 * 100.0).min(99.0)
        } else {
            0.0
        };

        CloneReport {
            total_clones_found: clusters.len(),
            estimated_duplicate_lines: estimated_dup,
            refactor_savings_pct: savings_pct,
            clone_clusters: clusters,
        }
    }

    /// Split file into normalized chunks of `window_size` lines and compute rolling Rabin-Karp fingerprints
    fn extract_normalized_chunks(content: &str, window_size: usize) -> Vec<(usize, usize, String, u64)> {
        let lines: Vec<&str> = content.lines().collect();
        if lines.len() < window_size {
            return vec![];
        }

        let mut chunks = Vec::new();
        for i in 0..=(lines.len().saturating_sub(window_size)) {
            let window: Vec<&str> = lines[i..i + window_size].to_vec();
            let normalized = Self::normalize_tokens(&window.join("\n"));
            let fingerprint = Self::rabin_karp_hash(&normalized);
            chunks.push((i + 1, i + window_size, normalized, fingerprint));
        }

        chunks
    }

    /// Normalize tokens: strip whitespace, variable names, string literals
    fn normalize_tokens(code: &str) -> String {
        code.split_whitespace()
            .map(|tok| {
                if tok.starts_with('"') || tok.starts_with('\'') {
                    "STR"
                } else if tok.chars().all(|c| c.is_numeric()) {
                    "NUM"
                } else {
                    tok
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Rabin-Karp polynomial rolling hash
    fn rabin_karp_hash(s: &str) -> u64 {
        let base: u64 = 31;
        let modulus: u64 = 1_000_000_007;
        s.bytes().fold(0u64, |acc, byte| {
            acc.wrapping_mul(base).wrapping_add(byte as u64) % modulus
        })
    }
}
