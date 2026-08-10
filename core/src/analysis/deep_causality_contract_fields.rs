//! Field-level contract metadata extracted from explicit schema syntax.

use crate::analysis::deep_causality::*;
use crate::analysis::deep_causality_extractors::RepositoryArtifact;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

pub fn enrich_contract_fields(engine: &mut DeepCausalityEngine, artifacts: &[RepositoryArtifact]) {
    for artifact in artifacts {
        let lower = artifact.path.to_ascii_lowercase();
        if lower.ends_with(".graphql") || lower.ends_with(".gql") { enrich_graphql(engine, artifact); }
        if (lower.contains("openapi") || lower.contains("swagger")) && lower.ends_with(".json") { enrich_openapi_json(engine, artifact); }
        if lower.ends_with(".prisma") || lower.ends_with("schema.prisma") { enrich_prisma(engine, artifact); }
    }
}

fn enrich_graphql(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let mut schema: Option<String> = None;
    for raw in artifact.content.lines() {
        let line = raw.trim();
        let mut words = line.split_whitespace();
        let declaration = words.next().unwrap_or("");
        if matches!(declaration, "type" | "input" | "interface") {
            if let Some(name) = words.next() {
                schema = Some(format!("repo:{}::schema:{}", artifact.repository, name.trim_matches('{')));
                continue;
            }
        }
        if line == "}" { schema = None; continue; }
        let Some(schema_id) = schema.as_deref() else { continue; };
        let Some((left, right)) = line.split_once(':') else { continue; };
        let field = left.trim().split('(').next().unwrap_or("");
        if field.is_empty() { continue; }
        let type_token = right.trim().split_whitespace().next().unwrap_or("").trim_end_matches(',');
        if type_token.is_empty() { continue; }
        let required = type_token.ends_with('!');
        let type_name = type_token.trim_end_matches('!').to_string();
        let field_id = format!("{schema_id}::field:{field}");
        upsert_field(engine, &field_id, field, &artifact.repository, &artifact.path, &type_name, required);
        ensure_defines(engine, schema_id, &field_id, artifact);
    }
}

fn enrich_openapi_json(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let Ok(json) = serde_json::from_str::<Value>(&artifact.content) else { return; };
    let Some(schemas) = json.pointer("/components/schemas").and_then(Value::as_object) else { return; };
    for (schema_name, schema_value) in schemas {
        let schema_id = format!("repo:{}::schema:{}", artifact.repository, schema_name);
        ensure_schema(engine, &schema_id, schema_name, artifact);
        let required: HashSet<String> = schema_value.get("required")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default();
        let Some(properties) = schema_value.get("properties").and_then(Value::as_object) else { continue; };
        for (field, definition) in properties {
            let type_name = definition.get("type").and_then(Value::as_str)
                .or_else(|| definition.get("$ref").and_then(Value::as_str).map(|reference| reference.rsplit('/').next().unwrap_or(reference)))
                .unwrap_or("unknown");
            let field_id = format!("{schema_id}::field:{field}");
            upsert_field(engine, &field_id, field, &artifact.repository, &artifact.path, type_name, required.contains(field));
            ensure_defines(engine, &schema_id, &field_id, artifact);
        }
    }
}

fn enrich_prisma(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let mut table: Option<String> = None;
    for raw in artifact.content.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("model ") {
            let name = rest.split_whitespace().next().unwrap_or("").trim_matches('{');
            table = if name.is_empty() { None } else { Some(format!("repo:{}::table:{}", artifact.repository, name)) };
            continue;
        }
        if line == "}" { table = None; continue; }
        let Some(table_id) = table.as_deref() else { continue; };
        if line.is_empty() || line.starts_with("//") || line.starts_with("@@") { continue; }
        let mut parts = line.split_whitespace();
        let field = parts.next().unwrap_or("");
        let type_token = parts.next().unwrap_or("");
        if field.is_empty() || type_token.is_empty() { continue; }
        let required = !type_token.ends_with('?') && !type_token.ends_with("[]");
        let type_name = type_token.trim_end_matches('?').to_string();
        let field_id = format!("{table_id}::column:{field}");
        upsert_field(engine, &field_id, field, &artifact.repository, &artifact.path, &type_name, required);
        if let Some(mut entity) = engine.entities().find(|entity| entity.id == field_id).cloned() {
            entity.kind = CausalEntityKind::Column;
            engine.upsert_entity(entity);
        }
        ensure_defines(engine, table_id, &field_id, artifact);
    }
}

fn ensure_schema(engine: &mut DeepCausalityEngine, id: &str, name: &str, artifact: &RepositoryArtifact) {
    if engine.entities().any(|entity| entity.id == id) { return; }
    engine.upsert_entity(CausalEntity { id:id.to_string(), kind:CausalEntityKind::Schema, name:name.to_string(), repository:Some(artifact.repository.clone()), path:Some(artifact.path.clone()), attributes:BTreeMap::new() });
}

fn upsert_field(engine: &mut DeepCausalityEngine, id: &str, name: &str, repository: &str, path: &str, type_name: &str, required: bool) {
    let mut entity = engine.entities().find(|entity| entity.id == id).cloned().unwrap_or_else(|| CausalEntity {
        id:id.to_string(), kind:CausalEntityKind::Value, name:name.to_string(), repository:Some(repository.to_string()), path:Some(path.to_string()), attributes:BTreeMap::new()
    });
    entity.attributes.insert("contract.type".into(), type_name.to_string());
    entity.attributes.insert("contract.required".into(), required.to_string());
    engine.upsert_entity(entity);
}

fn ensure_defines(engine: &mut DeepCausalityEngine, parent: &str, child: &str, artifact: &RepositoryArtifact) {
    if engine.facts().iter().any(|fact| fact.from == parent && fact.to == child && fact.relation == CausalRelationKind::Defines) { return; }
    if !engine.entities().any(|entity| entity.id == parent) || !engine.entities().any(|entity| entity.id == child) { return; }
    let _ = engine.add_fact(CausalFact {
        from:parent.to_string(), to:child.to_string(), relation:CausalRelationKind::Defines,
        evidence:CausalEvidenceClass::Static, confidence:1.0, condition:None, timestamp_ms:None,
        metadata:BTreeMap::from([("source.path".into(),artifact.path.clone())]),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphql_non_null_field_is_required() {
        let mut engine=DeepCausalityEngine::new();
        let artifact=RepositoryArtifact{repository:"r".into(),path:"schema.graphql".into(),content:"type User {\n id: ID!\n name: String\n}".into()};
        enrich_contract_fields(&mut engine,&[artifact]);
        let id=engine.entities().find(|entity|entity.id=="repo:r::schema:User::field:id").unwrap();
        assert_eq!(id.attributes.get("contract.required").map(String::as_str),Some("true"));
    }
}
