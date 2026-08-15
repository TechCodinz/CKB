//! Additional manifest identity/dependency extraction for Go and Python.

use crate::analysis::deep_causality::*;
use crate::analysis::deep_causality_extractors::RepositoryArtifact;
use std::collections::BTreeMap;

pub fn enrich_extra_manifests(engine: &mut DeepCausalityEngine, artifacts: &[RepositoryArtifact]) {
    for artifact in artifacts {
        let lower = artifact.path.to_ascii_lowercase();
        if lower.ends_with("go.mod") { extract_go_mod(engine, artifact); }
        if lower.ends_with("pyproject.toml") { extract_pyproject(engine, artifact); }
    }
}

fn extract_go_mod(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let file = ensure_file(engine, artifact);
    let mut module: Option<String> = None;
    let mut in_require = false;
    let mut dependencies: Vec<(String, usize)> = Vec::new();
    for (index, raw) in artifact.content.lines().enumerate() {
        let line = raw.trim();
        if let Some(name) = line.strip_prefix("module ") {
            let name = name.trim();
            if !name.is_empty() { module = Some(name.to_string()); }
            continue;
        }
        if line == "require (" { in_require = true; continue; }
        if in_require && line == ")" { in_require = false; continue; }
        if let Some(rest) = line.strip_prefix("require ") {
            if let Some(name) = rest.split_whitespace().next() { if !name.is_empty() { dependencies.push((name.to_string(), index + 1)); } }
            continue;
        }
        if in_require {
            let name = line.split_whitespace().next().unwrap_or("");
            if !name.is_empty() && !name.starts_with("//") { dependencies.push((name.to_string(), index + 1)); }
        }
    }
    let Some(module) = module else { return; };
    let source = repo_package(&artifact.repository, &module);
    ensure_package(engine, &source, &module, Some(&artifact.repository));
    add(engine, fact(&file, &source, CausalRelationKind::Defines, artifact, 1));
    for (dependency, line) in dependencies {
        let target = external_package(&dependency);
        ensure_package(engine, &target, &dependency, None);
        add(engine, fact(&source, &target, CausalRelationKind::DependsOn, artifact, line));
    }
}

fn extract_pyproject(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let file = ensure_file(engine, artifact);
    let mut section = String::new();
    let mut project_name: Option<String> = None;
    let mut dependencies: Vec<(String, usize)> = Vec::new();
    let mut in_project_dependency_array = false;

    for (index, raw) in artifact.content.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') && !line.starts_with("[[") {
            section = line.trim_matches(|character| character == '[' || character == ']').to_string();
            in_project_dependency_array = false;
            continue;
        }
        if matches!(section.as_str(), "project" | "tool.poetry") && line.starts_with("name") {
            if let Some((_, value)) = line.split_once('=') {
                let name = unquote(value.trim());
                if !name.is_empty() { project_name = Some(name.to_string()); }
            }
        }
        if section == "project" && line.starts_with("dependencies") {
            if let Some((_, value)) = line.split_once('=') {
                let value = value.trim();
                if value.starts_with('[') {
                    for quoted in quoted_values(value) {
                        if let Some(name) = python_requirement_name(&quoted) { dependencies.push((name, index + 1)); }
                    }
                    in_project_dependency_array = !value.contains(']');
                }
            }
            continue;
        }
        if in_project_dependency_array {
            for quoted in quoted_values(line) {
                if let Some(name) = python_requirement_name(&quoted) { dependencies.push((name, index + 1)); }
            }
            if line.contains(']') { in_project_dependency_array = false; }
        }
        if section == "tool.poetry.dependencies" {
            if let Some((name, _)) = line.split_once('=') {
                let name = name.trim();
                if !name.is_empty() && !name.eq_ignore_ascii_case("python") { dependencies.push((name.to_string(), index + 1)); }
            }
        }
    }

    let Some(project_name) = project_name else { return; };
    let source = repo_package(&artifact.repository, &project_name);
    ensure_package(engine, &source, &project_name, Some(&artifact.repository));
    add(engine, fact(&file, &source, CausalRelationKind::Defines, artifact, 1));
    dependencies.sort(); dependencies.dedup();
    for (dependency, line) in dependencies {
        let target = external_package(&dependency);
        ensure_package(engine, &target, &dependency, None);
        add(engine, fact(&source, &target, CausalRelationKind::DependsOn, artifact, line));
    }
}

fn python_requirement_name(value: &str) -> Option<String> {
    let base = value.split(';').next()?.trim();
    let stop = base.find(|character: char| matches!(character, '<' | '>' | '=' | '!' | '~' | '[' | ' ')).unwrap_or(base.len());
    let name = base[..stop].trim();
    if name.is_empty() { None } else { Some(name.to_string()) }
}

fn quoted_values(line: &str) -> Vec<String> {
    let mut values = Vec::new();
    for quote in ['"', '\''] {
        let mut rest = line;
        while let Some(start) = rest.find(quote) {
            let tail = &rest[start + 1..];
            let Some(end) = tail.find(quote) else { break; };
            values.push(tail[..end].to_string());
            rest = &tail[end + 1..];
        }
    }
    values
}
fn unquote(value: &str) -> &str { value.trim().trim_matches(|character| character == '"' || character == '\'') }
fn repo_package(repository: &str, name: &str) -> String { format!("repo:{repository}::package:{name}") }
fn external_package(name: &str) -> String { format!("package:external:{name}") }

fn ensure_file(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) -> String {
    let id = format!("repo:{}::file:{}", artifact.repository, normalize(&artifact.path));
    if !engine.entities().any(|entity| entity.id == id) {
        engine.upsert_entity(CausalEntity { id:id.clone(), kind:CausalEntityKind::File, name:artifact.path.clone(), repository:Some(artifact.repository.clone()), path:Some(normalize(&artifact.path)), attributes:BTreeMap::new() });
    }
    id
}
fn ensure_package(engine: &mut DeepCausalityEngine, id: &str, name: &str, repository: Option<&str>) {
    if !engine.entities().any(|entity| entity.id == id) {
        engine.upsert_entity(CausalEntity { id:id.to_string(), kind:CausalEntityKind::Package, name:name.to_string(), repository:repository.map(str::to_string), path:None, attributes:BTreeMap::from([("package.name".into(),name.to_string())]) });
    }
}
fn fact(from:&str,to:&str,relation:CausalRelationKind,artifact:&RepositoryArtifact,line:usize)->CausalFact {
    CausalFact{from:from.to_string(),to:to.to_string(),relation,evidence:CausalEvidenceClass::Static,confidence:1.0,condition:None,timestamp_ms:None,metadata:BTreeMap::from([("source.path".into(),artifact.path.clone()),("source.line".into(),line.to_string())])}
}
fn add(engine:&mut DeepCausalityEngine,fact:CausalFact){ if !engine.facts().iter().any(|existing|existing==&fact){let _=engine.add_fact(fact);} }
fn normalize(path:&str)->String{path.replace('\\',"/").trim_start_matches("./").to_string()}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn parses_python_requirement_names(){ assert_eq!(python_requirement_name("fastapi>=0.100"),Some("fastapi".into())); assert_eq!(python_requirement_name("uvicorn[standard]~=0.30"),Some("uvicorn".into())); }
}
