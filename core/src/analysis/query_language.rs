//! CKB Architecture Query Language (AQL) V1.
//!
//! Models may generate AQL, but deterministic CKB code resolves the operation.
//! Natural language safely falls back to bounded Architecture Memory retrieval.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

pub const AQL_VERSION: &str = "ckb-aql-v1";
pub const MAX_AQL_CHARS: usize = 12_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ArchitectureQueryOperation {
    Memory { query: String, depth: usize, limit: usize },
    Path { source: String, target: String, depth: usize },
    Dependents { symbol_id: String, depth: usize },
    Impact { file: String, line: usize },
    Dna { symbol_id: Option<String> },
    History { snapshot_id: Option<String> },
    Diff { from_snapshot: String, to_snapshot: String },
    Runtime { identity: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchitectureQueryManifest {
    pub version: String,
    pub operations: Vec<String>,
    pub natural_language_fallback: String,
    pub evidence_policy: String,
    pub synthetic: bool,
}

pub fn architecture_query_manifest() -> ArchitectureQueryManifest {
    ArchitectureQueryManifest {
        version: AQL_VERSION.into(),
        operations: vec![
            "MEMORY <query> [DEPTH n] [LIMIT n]".into(),
            "PATH <source-symbol-id> -> <target-symbol-id> [DEPTH n]".into(),
            "DEPENDENTS <symbol-id> [DEPTH n]".into(),
            "IMPACT <source-path>[:line]".into(),
            "DNA [symbol-id]".into(),
            "HISTORY [snapshot-id]".into(),
            "DIFF <from-snapshot> -> <to-snapshot>".into(),
            "RUNTIME [symbol-id|trace-id]".into(),
        ],
        natural_language_fallback: "MEMORY".into(),
        evidence_policy: "static-runtime-predicted-separated".into(),
        synthetic: false,
    }
}

fn bounded(value: Option<&str>, fallback: usize, max: usize) -> usize {
    value.and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(fallback)
        .clamp(1, max)
}

fn option_value<'a>(tokens: &'a [&'a str], name: &str) -> Option<&'a str> {
    tokens.windows(2)
        .find(|window| window[0].eq_ignore_ascii_case(name))
        .map(|window| window[1])
}

fn strip_numeric_options(tokens: &[&str]) -> String {
    let mut out = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if (tokens[index].eq_ignore_ascii_case("DEPTH") || tokens[index].eq_ignore_ascii_case("LIMIT"))
            && index + 1 < tokens.len()
            && tokens[index + 1].parse::<usize>().is_ok()
        {
            index += 2;
            continue;
        }
        out.push(tokens[index]);
        index += 1;
    }
    out.join(" ")
}

fn split_arrow(body: &str, syntax: &str) -> Result<(String, String)> {
    let parts = body.split("->").map(str::trim).filter(|item| !item.is_empty()).collect::<Vec<_>>();
    if parts.len() != 2 { return Err(anyhow!(syntax.to_string())); }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

fn parse_impact(body: &str) -> Result<(String, usize)> {
    if body.is_empty() { return Err(anyhow!("IMPACT requires a repository-relative source path")); }
    if let Some((file, line)) = body.rsplit_once(':') {
        if let Ok(line) = line.parse::<usize>() {
            let file = file.trim();
            if file.is_empty() { return Err(anyhow!("IMPACT requires a repository-relative source path")); }
            return Ok((file.to_string(), line.max(1)));
        }
    }
    Ok((body.to_string(), 1))
}

pub fn parse_architecture_query(input: &str) -> Result<ArchitectureQueryOperation> {
    let raw = input.trim();
    if raw.is_empty() { return Err(anyhow!("architecture query is required")); }
    if raw.chars().count() > MAX_AQL_CHARS { return Err(anyhow!("architecture query is too large")); }

    let tokens = raw.split_whitespace().collect::<Vec<_>>();
    let command = tokens.first().map(|value| value.to_ascii_uppercase()).unwrap_or_default();
    let recognized = matches!(command.as_str(), "MEMORY" | "PATH" | "DEPENDENTS" | "IMPACT" | "DNA" | "HISTORY" | "DIFF" | "RUNTIME");
    let depth = bounded(option_value(&tokens, "DEPTH"), 12, 32);
    let limit = bounded(option_value(&tokens, "LIMIT"), 32, 250);

    if !recognized {
        return Ok(ArchitectureQueryOperation::Memory { query: raw.to_string(), depth: depth.min(8), limit });
    }

    let body = strip_numeric_options(&tokens[1..]);
    match command.as_str() {
        "MEMORY" => Ok(ArchitectureQueryOperation::Memory {
            query: if body.is_empty() { "architecture overview".into() } else { body },
            depth: depth.min(8),
            limit,
        }),
        "PATH" => {
            let (source, target) = split_arrow(&body, "PATH syntax: PATH <source-symbol-id> -> <target-symbol-id> [DEPTH n]")?;
            Ok(ArchitectureQueryOperation::Path { source, target, depth })
        }
        "DEPENDENTS" => {
            if body.is_empty() { return Err(anyhow!("DEPENDENTS requires a stable symbol ID")); }
            Ok(ArchitectureQueryOperation::Dependents { symbol_id: body, depth })
        }
        "IMPACT" => {
            let (file, line) = parse_impact(&body)?;
            Ok(ArchitectureQueryOperation::Impact { file, line })
        }
        "DNA" => Ok(ArchitectureQueryOperation::Dna { symbol_id: (!body.is_empty()).then_some(body) }),
        "HISTORY" => Ok(ArchitectureQueryOperation::History { snapshot_id: (!body.is_empty()).then_some(body) }),
        "DIFF" => {
            let (from_snapshot, to_snapshot) = split_arrow(&body, "DIFF syntax: DIFF <from-snapshot> -> <to-snapshot>")?;
            Ok(ArchitectureQueryOperation::Diff { from_snapshot, to_snapshot })
        }
        "RUNTIME" => Ok(ArchitectureQueryOperation::Runtime { identity: (!body.is_empty()).then_some(body) }),
        _ => unreachable!("recognized command already matched"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_language_falls_back_to_memory() {
        let parsed = parse_architecture_query("why does checkout depend on inventory?").unwrap();
        assert_eq!(parsed, ArchitectureQueryOperation::Memory {
            query: "why does checkout depend on inventory?".into(), depth: 8, limit: 32,
        });
    }

    #[test]
    fn parses_multiple_options_in_any_order() {
        let parsed = parse_architecture_query("MEMORY checkout LIMIT 40 DEPTH 3").unwrap();
        assert_eq!(parsed, ArchitectureQueryOperation::Memory { query: "checkout".into(), depth: 3, limit: 40 });
    }

    #[test]
    fn parses_path_without_model_interpretation() {
        let parsed = parse_architecture_query("PATH src/a.ts::start -> src/b.ts::work DEPTH 7").unwrap();
        assert_eq!(parsed, ArchitectureQueryOperation::Path {
            source: "src/a.ts::start".into(), target: "src/b.ts::work".into(), depth: 7,
        });
    }

    #[test]
    fn impact_preserves_colons_in_windows_like_paths_except_numeric_tail() {
        let parsed = parse_architecture_query("IMPACT src/http:client.ts:44").unwrap();
        assert_eq!(parsed, ArchitectureQueryOperation::Impact { file: "src/http:client.ts".into(), line: 44 });
    }
}
