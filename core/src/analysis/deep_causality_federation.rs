//! Multi-repository federation for V13.1 Deep Software Causality.
//!
//! Federation does not invent organizational edges. Each workspace is scanned
//! independently, then evidence is merged. Cross-repository dependency edges
//! are materialized only when an observed manifest dependency name exactly
//! matches a package identity declared by another scanned repository.

use crate::analysis::{
    build_workspace_deep_causality, merge_deep_causality_evidence, CausalEntityKind,
    CausalEvidenceClass, CausalFact, CausalRelationKind, DeepCausalityEngine,
    WorkspaceCausalityReport,
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedWorkspaceMember {
    pub root: String,
    pub repository: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedCausalityReport {
    pub members: Vec<WorkspaceCausalityReport>,
    pub repositories: usize,
    pub causal_entities: usize,
    pub causal_facts: usize,
    pub resolved_cross_repo_dependencies: usize,
}

pub async fn build_federated_deep_causality(
    members: &[FederatedWorkspaceMember],
) -> Result<(DeepCausalityEngine, FederatedCausalityReport)> {
    if members.is_empty() { return Err(anyhow!("at least one federated workspace member is required")); }
    let mut engine = DeepCausalityEngine::new();
    let mut reports = Vec::new();

    for member in members {
        let root = PathBuf::from(&member.root);
        let (workspace, report) = build_workspace_deep_causality(&root, member.repository.clone()).await?;
        merge_deep_causality_evidence(&mut engine, &workspace).map_err(|e| anyhow!(e))?;
        reports.push(report);
    }

    let resolved_cross_repo_dependencies = resolve_manifest_cross_repo_dependencies(&mut engine);
    let report = FederatedCausalityReport {
        repositories: reports.len(),
        members: reports,
        causal_entities: engine.entities().count(),
        causal_facts: engine.facts().len(),
        resolved_cross_repo_dependencies,
    };
    Ok((engine, report))
}

fn resolve_manifest_cross_repo_dependencies(engine: &mut DeepCausalityEngine) -> usize {
    let declared: HashMap<String, Vec<(String, String)>> = engine.entities()
        .filter(|e| e.kind == CausalEntityKind::Package && e.repository.is_some())
        .filter_map(|e| {
            let name = e.attributes.get("package.name").cloned().unwrap_or_else(|| e.name.clone());
            Some((name, (e.id.clone(), e.repository.clone()?)))
        })
        .fold(HashMap::new(), |mut map, (name, value)| { map.entry(name).or_default().push(value); map });

    let external_names: HashMap<String, String> = engine.entities()
        .filter(|e| e.kind == CausalEntityKind::Package && e.repository.is_none() && e.id.starts_with("package:external:"))
        .map(|e| (e.id.clone(), e.attributes.get("package.name").cloned().unwrap_or_else(|| e.name.clone())))
        .collect();

    let observed_dependencies: Vec<CausalFact> = engine.facts().iter()
        .filter(|f| f.relation == CausalRelationKind::DependsOn && external_names.contains_key(&f.to))
        .cloned()
        .collect();

    let entity_repos: HashMap<String, Option<String>> = engine.entities().map(|e| (e.id.clone(), e.repository.clone())).collect();
    let mut additions = Vec::new();
    for observed in observed_dependencies {
        let Some(name) = external_names.get(&observed.to) else { continue; };
        let source_repo = entity_repos.get(&observed.from).and_then(|v| v.clone());
        for (target, target_repo) in declared.get(name).into_iter().flatten() {
            if source_repo.as_deref() == Some(target_repo.as_str()) { continue; }
            if observed.from == *target { continue; }
            let duplicate = engine.facts().iter().any(|f| f.from == observed.from && f.to == *target && f.relation == CausalRelationKind::DependsOn);
            if duplicate { continue; }
            let mut metadata = BTreeMap::new();
            metadata.insert("federated.resolution".into(), "exact_manifest_package_name".into());
            metadata.insert("package.name".into(), name.clone());
            if let Some(section) = observed.metadata.get("manifest.section") { metadata.insert("manifest.section".into(), section.clone()); }
            additions.push(CausalFact {
                from: observed.from.clone(),
                to: target.clone(),
                relation: CausalRelationKind::DependsOn,
                evidence: CausalEvidenceClass::Static,
                confidence: observed.confidence,
                condition: observed.condition.clone(),
                timestamp_ms: observed.timestamp_ms,
                metadata,
            });
        }
    }
    let count = additions.len();
    for fact in additions { let _ = engine.add_fact(fact); }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::CausalEntity;

    #[test]
    fn exact_manifest_name_resolves_cross_repo_dependency() {
        let mut engine = DeepCausalityEngine::new();
        engine.upsert_entity(CausalEntity { id:"repo:a::package:app".into(), kind:CausalEntityKind::Package, name:"app".into(), repository:Some("a".into()), path:None, attributes:BTreeMap::from([("package.name".into(),"app".into())]) });
        engine.upsert_entity(CausalEntity { id:"repo:b::package:shared".into(), kind:CausalEntityKind::Package, name:"shared".into(), repository:Some("b".into()), path:None, attributes:BTreeMap::from([("package.name".into(),"shared".into())]) });
        engine.upsert_entity(CausalEntity { id:"package:external:shared".into(), kind:CausalEntityKind::Package, name:"shared".into(), repository:None, path:None, attributes:BTreeMap::from([("package.name".into(),"shared".into())]) });
        engine.add_fact(CausalFact { from:"repo:a::package:app".into(), to:"package:external:shared".into(), relation:CausalRelationKind::DependsOn, evidence:CausalEvidenceClass::Static, confidence:1.0, condition:None, timestamp_ms:None, metadata:BTreeMap::new() }).unwrap();
        assert_eq!(resolve_manifest_cross_repo_dependencies(&mut engine),1);
        assert!(engine.cross_repo_path("repo:a::package:app","repo:b::package:shared",4).is_some());
    }
}
