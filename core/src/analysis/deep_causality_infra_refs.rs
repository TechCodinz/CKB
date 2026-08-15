//! Explicit infrastructure dependency references for Terraform/HCL artifacts.

use crate::analysis::deep_causality::*;
use crate::analysis::deep_causality_extractors::RepositoryArtifact;
use std::collections::BTreeMap;

pub fn enrich_infrastructure_references(engine: &mut DeepCausalityEngine, artifacts: &[RepositoryArtifact]) {
    for artifact in artifacts {
        if !artifact.path.to_ascii_lowercase().ends_with(".tf") { continue; }
        let file = ensure_file(engine, artifact);
        let mut current: Option<String> = None;
        for (index, raw) in artifact.content.lines().enumerate() {
            let line = raw.trim();
            if line.starts_with("resource ") || line.starts_with("module ") || line.starts_with("data ") {
                let quoted: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
                if !quoted.is_empty() {
                    let name = quoted.join(".");
                    let id = format!("repo:{}::infra:{}", artifact.repository, name);
                    ensure_infra(engine, &id, &name, artifact);
                    add(engine, fact(&file, &id, CausalRelationKind::Defines, artifact, index + 1, "declaration"));
                    current = Some(id);
                }
                continue;
            }
            if line == "}" { current = None; continue; }
            let Some(source) = current.as_deref() else { continue; };
            if let Some((_, value)) = line.split_once("depends_on") {
                for reference in terraform_references(value) {
                    let target = infra_reference_id(&artifact.repository, &reference);
                    ensure_infra(engine, &target, &reference, artifact);
                    add(engine, fact(source, &target, CausalRelationKind::DependsOn, artifact, index + 1, "depends_on"));
                }
                continue;
            }
            for reference in terraform_references(line) {
                if reference.starts_with("var.") || reference.starts_with("local.") || reference.starts_with("each.") || reference.starts_with("count.") { continue; }
                let target = infra_reference_id(&artifact.repository, &reference);
                if target == source { continue; }
                ensure_infra(engine, &target, &reference, artifact);
                add(engine, fact(source, &target, CausalRelationKind::DependsOn, artifact, index + 1, "expression_reference"));
            }
        }
    }
}

fn terraform_references(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in text.split(|character: char| character.is_whitespace() || matches!(character, '[' | ']' | '(' | ')' | '{' | '}' | ',' | '=' | ':' | '"' | '\'')) {
        let clean = token.trim().trim_matches(|character: char| matches!(character, '$' | '.'));
        let parts: Vec<&str> = clean.split('.').collect();
        if parts.len() < 2 { continue; }
        let reference = if parts[0] == "module" || parts[0] == "data" {
            parts.iter().take(3).copied().collect::<Vec<_>>().join(".")
        } else {
            parts.iter().take(2).copied().collect::<Vec<_>>().join(".")
        };
        if !reference.is_empty() { out.push(reference); }
    }
    out.sort(); out.dedup(); out
}

fn infra_reference_id(repository: &str, reference: &str) -> String { format!("repo:{repository}::infra:{reference}") }
fn ensure_file(engine:&mut DeepCausalityEngine,artifact:&RepositoryArtifact)->String{
    let id=format!("repo:{}::file:{}",artifact.repository,artifact.path.replace('\\',"/"));
    if !engine.entities().any(|entity|entity.id==id){engine.upsert_entity(CausalEntity{id:id.clone(),kind:CausalEntityKind::Infrastructure,name:artifact.path.clone(),repository:Some(artifact.repository.clone()),path:Some(artifact.path.clone()),attributes:BTreeMap::new()});}
    id
}
fn ensure_infra(engine:&mut DeepCausalityEngine,id:&str,name:&str,artifact:&RepositoryArtifact){if !engine.entities().any(|entity|entity.id==id){engine.upsert_entity(CausalEntity{id:id.to_string(),kind:CausalEntityKind::Infrastructure,name:name.to_string(),repository:Some(artifact.repository.clone()),path:Some(artifact.path.clone()),attributes:BTreeMap::new()});}}
fn fact(from:&str,to:&str,relation:CausalRelationKind,artifact:&RepositoryArtifact,line:usize,basis:&str)->CausalFact{CausalFact{from:from.to_string(),to:to.to_string(),relation,evidence:CausalEvidenceClass::Static,confidence:1.0,condition:None,timestamp_ms:None,metadata:BTreeMap::from([("source.path".into(),artifact.path.clone()),("source.line".into(),line.to_string()),("infra.basis".into(),basis.to_string())])}}
fn add(engine:&mut DeepCausalityEngine,fact:CausalFact){if !engine.facts().iter().any(|existing|existing==&fact){let _=engine.add_fact(fact);}}

#[cfg(test)]
mod tests{use super::*;#[test]fn extracts_resource_reference(){assert!(terraform_references("subnet_id = aws_subnet.private.id").contains(&"aws_subnet.private".to_string()));}}
