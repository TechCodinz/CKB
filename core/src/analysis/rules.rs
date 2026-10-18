//! Project architecture rules from `.ckb/rules.json`.
//!
//! `ckb suggest` has always told users to "add a .ckb/rules.json file to
//! enforce boundaries", but nothing in the engine ever read that file —
//! scans silently ignored it, so a project could carry a perfectly valid
//! rules file and get zero enforcement. This module is the loader and the
//! edge-based checker the scan pipeline now runs.
//!
//! File format (an array of rule objects):
//!
//! ```json
//! [
//!   {
//!     "name": "ui-must-not-import-db",
//!     "description": "UI layer must go through the service layer, never the DB layer directly",
//!     "from": "src/ui/",
//!     "to": "src/db/",
//!     "severity": "Error"
//!   }
//! ]
//! ```
//!
//! * `from` (optional): substring matched against the *importing* file's
//!   path. When omitted, the rule applies to every importer outside the
//!   target area.
//! * `to` (optional): substring matched against the *imported* file's path.
//! * `pattern` (legacy, optional): the field the original `ckb validate`
//!   command matched. When `to` is absent, `pattern` is matched against
//!   the imported file's path, so existing validate-era rules files keep
//!   working. A rule needs at least one of `to`/`pattern`.
//! * `severity`: one of `Info`, `Warning`, `Error`, `Critical`.
//!
//! A rule fires on a file-level `Import` edge whose source matches `from`
//! (or, without `from`, whose source is not itself inside the target
//! area) and whose target matches `to`/`pattern`. Intra-area imports
//! never fire: a layer importing itself is not a boundary crossing.
//!
//! A rules file that exists but cannot be parsed is a hard error, not a
//! silent skip — silent non-enforcement is exactly the defect this fixes.

use crate::graph::DependencyGraph;
use crate::types::*;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

/// One rule from `.ckb/rules.json` (see module docs for the format).
#[derive(Debug, Clone, Deserialize)]
pub struct ProjectRule {
    pub name: String,
    pub description: String,
    /// Legacy target matcher (validate-era files); used when `to` is absent.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Substring matched against the importing file's path.
    #[serde(default)]
    pub from: Option<String>,
    /// Substring matched against the imported file's path.
    #[serde(default)]
    pub to: Option<String>,
    pub severity: Severity,
}

impl ProjectRule {
    /// The substring matched against the imported file's path, if any.
    fn target_pattern(&self) -> Option<&str> {
        self.to.as_deref().or(self.pattern.as_deref())
    }
}

/// Load `<root>/.ckb/rules.json`. Returns `Ok(None)` when the file does
/// not exist (rules are opt-in) and `Err` when it exists but is invalid —
/// a project that wrote rules must never be silently unenforced.
pub fn load_project_rules(root: &Path) -> Result<Option<Vec<ProjectRule>>> {
    let file = root.join(".ckb").join("rules.json");
    if !file.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&file)
        .with_context(|| format!("Failed to read architecture rules file {}", file.display()))?;
    let rules: Vec<ProjectRule> = serde_json::from_str(&content).with_context(|| {
        format!(
            "Failed to parse architecture rules file {} \
             (expected a JSON array of {{name, description, from/to or pattern, severity}})",
            file.display()
        )
    })?;
    Ok(Some(rules))
}

/// Check file-level import edges against the project rules. One
/// violation per (rule, importer, imported) triple.
pub fn check_project_rules(graph: &DependencyGraph, rules: &[ProjectRule]) -> Vec<DriftViolation> {
    let nodes_by_id: std::collections::HashMap<&NodeId, &Node> =
        graph.nodes().into_iter().map(|n| (&n.id, n)).collect();
    let mut violations = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for edge in graph.edges() {
        if edge.kind != EdgeKind::Import {
            continue;
        }
        let (Some(from_node), Some(to_node)) =
            (nodes_by_id.get(&edge.from), nodes_by_id.get(&edge.to))
        else {
            continue;
        };
        if from_node.kind != NodeKind::File || to_node.kind != NodeKind::File {
            continue;
        }
        let from_path = from_node.path.to_string_lossy();
        let to_path = to_node.path.to_string_lossy();

        for rule in rules {
            let Some(target) = rule.target_pattern() else {
                continue;
            };
            if !to_path.contains(target) {
                continue;
            }
            match &rule.from {
                Some(from_pat) => {
                    if !from_path.contains(from_pat.as_str()) {
                        continue;
                    }
                }
                // No explicit source scope: imports from inside the
                // target area itself are not crossings.
                None => {
                    if from_path.contains(target) {
                        continue;
                    }
                }
            }

            let key = format!("{}|{}|{}", rule.name, edge.from.0, edge.to.0);
            if !seen.insert(key) {
                continue;
            }
            violations.push(DriftViolation {
                id: uuid::Uuid::new_v4(),
                kind: ViolationKind::ForbiddenDependency,
                from: edge.from.clone(),
                to: edge.to.clone(),
                boundary: rule.name.clone(),
                message: format!(
                    "{} imports {} — forbidden by .ckb/rules.json rule '{}': {}",
                    from_path, to_path, rule.name, rule.description
                ),
                severity: rule.severity,
                suggested_fix: Some(format!(
                    "Remove the direct dependency from {} to {}, or route it through an allowed layer (rule '{}')",
                    from_path, to_path, rule.name
                )),
            });
        }
    }

    violations
}
