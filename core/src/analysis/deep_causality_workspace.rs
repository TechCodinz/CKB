//! Workspace builder for V13.1 Deep Software Causality.

use crate::analysis::{build_deep_causality_bundle, DeepCausalityEngine, RepositoryArtifact};
use crate::{DependencyGraph, FileAnalysis, LanguageParser};
use anyhow::{Context, Result};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use std::path::Path;

const MAX_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ARTIFACTS: usize = 25_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceCausalityReport {
    pub root: String,
    pub repository: String,
    pub parsed_source_files: usize,
    pub artifact_files: usize,
    pub skipped_large_artifacts: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub causal_entities: usize,
    pub causal_facts: usize,
}

pub async fn build_workspace_deep_causality(
    root: impl AsRef<Path>,
    repository: impl Into<String>,
) -> Result<(DeepCausalityEngine, WorkspaceCausalityReport)> {
    let root = root.as_ref().canonicalize().with_context(|| format!("canonicalize {}", root.as_ref().display()))?;
    let repository = repository.into();
    let parser = LanguageParser::new();
    let mut analyses: Vec<FileAnalysis> = Vec::new();
    let mut artifacts = Vec::new();
    let mut skipped_large_artifacts = 0usize;

    for result in WalkBuilder::new(&root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .follow_links(false)
        .build()
    {
        let entry = match result { Ok(v) => v, Err(_) => continue };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) { continue; }
        let path = entry.path();
        let relative = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/");

        if supported_source(path) {
            let absolute = path.to_string_lossy().to_string();
            if let Ok(analysis) = parser.parse_file(&absolute).await {
                analyses.push(analysis);
            }
        }

        if artifacts.len() < MAX_ARTIFACTS && causal_artifact_candidate(path, &relative) {
            match std::fs::metadata(path) {
                Ok(meta) if meta.len() <= MAX_ARTIFACT_BYTES => {
                    if let Ok(content) = std::fs::read_to_string(path) {
                        artifacts.push(RepositoryArtifact { repository: repository.clone(), path: relative, content });
                    }
                }
                Ok(_) => skipped_large_artifacts += 1,
                Err(_) => {}
            }
        }
    }

    let mut graph = DependencyGraph::new();
    for analysis in &analyses { graph.add_file(analysis)?; }
    graph.build_call_graph()?;
    graph.build_type_graph()?;

    let engine = build_deep_causality_bundle(&graph, repository.clone(), &artifacts);
    let report = WorkspaceCausalityReport {
        root: root.to_string_lossy().replace('\\', "/"),
        repository,
        parsed_source_files: analyses.len(),
        artifact_files: artifacts.len(),
        skipped_large_artifacts,
        graph_nodes: graph.node_count(),
        graph_edges: graph.edge_count(),
        causal_entities: engine.entities().count(),
        causal_facts: engine.facts().len(),
    };
    Ok((engine, report))
}

fn supported_source(path: &Path) -> bool {
    matches!(path.extension().and_then(|v| v.to_str()).map(|v| v.to_ascii_lowercase()).as_deref(),
        Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "py" | "go" | "rs" | "java"))
}

fn causal_artifact_candidate(path: &Path, relative: &str) -> bool {
    if supported_source(path) { return true; }
    let lower = relative.to_ascii_lowercase();
    if lower.ends_with("codeowners") || lower.ends_with("dockerfile") || lower.contains("/.github/workflows/") || lower.starts_with(".github/workflows/") { return true; }
    matches!(path.extension().and_then(|v| v.to_str()).map(|v| v.to_ascii_lowercase()).as_deref(),
        Some("prisma" | "sql" | "tf" | "yaml" | "yml" | "json" | "toml" | "env" | "properties" | "xml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_selection_includes_architecture_files() {
        assert!(causal_artifact_candidate(Path::new("prisma/schema.prisma"), "prisma/schema.prisma"));
        assert!(causal_artifact_candidate(Path::new("docker-compose.yml"), "docker-compose.yml"));
        assert!(causal_artifact_candidate(Path::new(".github/CODEOWNERS"), ".github/CODEOWNERS"));
        assert!(!causal_artifact_candidate(Path::new("image.png"), "image.png"));
    }
}
