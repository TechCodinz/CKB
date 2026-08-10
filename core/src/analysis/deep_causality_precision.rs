//! Precision repository extraction for V13.1 Deep Software Causality.
//!
//! All relationships here come from identifiers explicitly present in files.
//! The pass intentionally avoids framework guesses when a target cannot be
//! resolved from observed repository artifacts.

use super::deep_causality::*;
use super::deep_causality_extractors::RepositoryArtifact;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path};

pub fn enrich_deep_artifact_semantics(engine: &mut DeepCausalityEngine, artifacts: &[RepositoryArtifact]) {
    let paths = collect_paths(artifacts);
    for artifact in artifacts {
        let lower = artifact.path.to_ascii_lowercase();
        if lower.ends_with("package.json") { extract_package_json(engine, artifact); }
        if lower.ends_with("cargo.toml") { extract_cargo_manifest(engine, artifact); }
        if is_openapi(&lower, &artifact.content) { extract_openapi(engine, artifact); }
        if lower.ends_with(".graphql") || lower.ends_with(".gql") { extract_graphql(engine, artifact); }
        if lower.ends_with(".proto") { extract_proto(engine, artifact); }
        if is_compose(&lower) { extract_compose(engine, artifact); }
        extract_data_access(engine, artifact);
        extract_config_guards(engine, artifact);
        if is_test(&lower) { extract_test_imports(engine, artifact, paths.get(&artifact.repository)); }
    }
}

fn collect_paths(artifacts: &[RepositoryArtifact]) -> HashMap<String, HashSet<String>> {
    let mut map: HashMap<String, HashSet<String>> = HashMap::new();
    for artifact in artifacts {
        map.entry(artifact.repository.clone()).or_default().insert(normalize(&artifact.path));
    }
    map
}

fn entity_exists(engine: &DeepCausalityEngine, id: &str) -> bool {
    engine.entities().any(|entity| entity.id == id)
}

fn ensure(engine: &mut DeepCausalityEngine, entity: CausalEntity) {
    if !entity_exists(engine, &entity.id) { engine.upsert_entity(entity); }
}

fn add(engine: &mut DeepCausalityEngine, fact: CausalFact) {
    if engine.facts().iter().any(|existing| existing == &fact) { return; }
    let _ = engine.add_fact(fact);
}

fn static_fact(from: &str, to: &str, relation: CausalRelationKind, artifact: &RepositoryArtifact, line: usize) -> CausalFact {
    CausalFact {
        from: from.to_string(),
        to: to.to_string(),
        relation,
        evidence: CausalEvidenceClass::Static,
        confidence: 1.0,
        condition: None,
        timestamp_ms: None,
        metadata: BTreeMap::from([
            ("source.path".to_string(), artifact.path.clone()),
            ("source.line".to_string(), line.to_string()),
        ]),
    }
}

fn artifact_file_id(artifact: &RepositoryArtifact) -> String {
    format!("repo:{}::file:{}", artifact.repository, normalize(&artifact.path))
}

fn ensure_file(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) -> String {
    let id = artifact_file_id(artifact);
    ensure(engine, CausalEntity {
        id: id.clone(),
        kind: if is_test(&artifact.path.to_ascii_lowercase()) { CausalEntityKind::Test } else { CausalEntityKind::File },
        name: artifact.path.rsplit('/').next().unwrap_or(&artifact.path).to_string(),
        repository: Some(artifact.repository.clone()),
        path: Some(normalize(&artifact.path)),
        attributes: BTreeMap::new(),
    });
    id
}

fn package_id(repo: &str, name: &str) -> String { format!("repo:{repo}::package:{name}") }
fn external_package_id(name: &str) -> String { format!("package:external:{name}") }

fn ensure_package(engine: &mut DeepCausalityEngine, id: String, name: &str, repo: Option<&str>) {
    ensure(engine, CausalEntity {
        id,
        kind: CausalEntityKind::Package,
        name: name.to_string(),
        repository: repo.map(str::to_string),
        path: None,
        attributes: BTreeMap::from([("package.name".to_string(), name.to_string())]),
    });
}

fn extract_package_json(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let Ok(json) = serde_json::from_str::<Value>(&artifact.content) else { return; };
    let Some(name) = json.get("name").and_then(Value::as_str) else { return; };
    let file = ensure_file(engine, artifact);
    let source = package_id(&artifact.repository, name);
    ensure_package(engine, source.clone(), name, Some(&artifact.repository));
    add(engine, static_fact(&file, &source, CausalRelationKind::Defines, artifact, 1));
    for section in ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"] {
        let Some(dependencies) = json.get(section).and_then(Value::as_object) else { continue; };
        for dependency in dependencies.keys() {
            let target = external_package_id(dependency);
            ensure_package(engine, target.clone(), dependency, None);
            let mut relation = static_fact(&source, &target, CausalRelationKind::DependsOn, artifact, 1);
            relation.metadata.insert("manifest.section".to_string(), section.to_string());
            add(engine, relation);
        }
    }
}

fn extract_cargo_manifest(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let file = ensure_file(engine, artifact);
    let mut section = String::new();
    let mut package_name: Option<String> = None;
    let mut dependencies: Vec<(String, String, usize)> = Vec::new();
    for (index, raw) in artifact.content.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line.trim_matches(|c| c == '[' || c == ']').to_string();
            continue;
        }
        if section == "package" && line.starts_with("name") {
            if let Some((_, value)) = line.split_once('=') {
                let value = value.trim().trim_matches('"');
                if !value.is_empty() { package_name = Some(value.to_string()); }
            }
        }
        if matches!(section.as_str(), "dependencies" | "dev-dependencies" | "build-dependencies") {
            if let Some((name, _)) = line.split_once('=') {
                let dependency = name.trim();
                if !dependency.is_empty() && !dependency.starts_with('#') {
                    dependencies.push((dependency.to_string(), section.clone(), index + 1));
                }
            }
        }
    }
    let Some(name) = package_name else { return; };
    let source = package_id(&artifact.repository, &name);
    ensure_package(engine, source.clone(), &name, Some(&artifact.repository));
    add(engine, static_fact(&file, &source, CausalRelationKind::Defines, artifact, 1));
    for (dependency, section, line) in dependencies {
        let target = external_package_id(&dependency);
        ensure_package(engine, target.clone(), &dependency, None);
        let mut relation = static_fact(&source, &target, CausalRelationKind::DependsOn, artifact, line);
        relation.metadata.insert("manifest.section".to_string(), section);
        add(engine, relation);
    }
}

fn is_openapi(path: &str, content: &str) -> bool {
    path.contains("openapi") || path.contains("swagger") || content.lines().take(8).any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("openapi:") || trimmed.starts_with("swagger:") || trimmed.contains("\"openapi\"")
    })
}

fn extract_openapi(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let file = ensure_file(engine, artifact);
    if artifact.path.to_ascii_lowercase().ends_with(".json") {
        if let Ok(json) = serde_json::from_str::<Value>(&artifact.content) {
            if let Some(paths) = json.get("paths").and_then(Value::as_object) {
                for (path, definition) in paths {
                    if let Some(methods) = definition.as_object() {
                        for method in methods.keys() {
                            if http_method(method) { add_api(engine, artifact, &file, method, path, 1); }
                        }
                    }
                }
            }
            if let Some(schemas) = json.pointer("/components/schemas").and_then(Value::as_object) {
                for name in schemas.keys() { add_schema(engine, artifact, &file, name, "openapi.schema", 1); }
            }
            return;
        }
    }

    let mut in_paths = false;
    let mut in_schemas = false;
    let mut path: Option<String> = None;
    for (index, raw) in artifact.content.lines().enumerate() {
        let indent = raw.chars().take_while(|character| character.is_whitespace()).count();
        let line = raw.trim();
        if line == "paths:" { in_paths = true; in_schemas = false; continue; }
        if line == "schemas:" { in_schemas = true; in_paths = false; continue; }
        if in_paths && indent == 2 && line.starts_with('/') && line.ends_with(':') {
            path = Some(line.trim_end_matches(':').to_string());
            continue;
        }
        if in_paths && indent == 4 && line.ends_with(':') {
            let method = line.trim_end_matches(':');
            if http_method(method) {
                if let Some(current) = path.as_deref() { add_api(engine, artifact, &file, method, current, index + 1); }
            }
        }
        if in_schemas && indent >= 4 && line.ends_with(':') {
            let name = line.trim_end_matches(':');
            if !matches!(name, "type" | "properties" | "required" | "items" | "description") {
                add_schema(engine, artifact, &file, name, "openapi.schema", index + 1);
            }
        }
    }
}

fn http_method(method: &str) -> bool {
    matches!(method.to_ascii_lowercase().as_str(), "get" | "post" | "put" | "patch" | "delete" | "options" | "head" | "trace")
}

fn add_api(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact, file: &str, method: &str, path: &str, line: usize) {
    let method_upper = method.to_ascii_uppercase();
    let id = format!("repo:{}::api:{}:{}", artifact.repository, method_upper, path);
    ensure(engine, CausalEntity {
        id: id.clone(), kind: CausalEntityKind::Api, name: format!("{} {}", method_upper, path),
        repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()),
        attributes: BTreeMap::from([("http.method".to_string(), method_upper), ("http.path".to_string(), path.to_string())]),
    });
    add(engine, static_fact(file, &id, CausalRelationKind::Defines, artifact, line));
}

fn add_schema(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact, file: &str, name: &str, kind: &str, line: usize) -> String {
    let id = format!("repo:{}::schema:{}", artifact.repository, name);
    ensure(engine, CausalEntity {
        id: id.clone(), kind: CausalEntityKind::Schema, name: name.to_string(),
        repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()),
        attributes: BTreeMap::from([("contract.kind".to_string(), kind.to_string())]),
    });
    add(engine, static_fact(file, &id, CausalRelationKind::Defines, artifact, line));
    id
}

fn extract_graphql(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let file = ensure_file(engine, artifact);
    let mut current: Option<String> = None;
    for (index, raw) in artifact.content.lines().enumerate() {
        let line = raw.trim();
        let mut words = line.split_whitespace();
        let declaration = words.next().unwrap_or("");
        if matches!(declaration, "type" | "input" | "interface" | "enum" | "union") {
            if let Some(name) = words.next() {
                let name = name.trim_matches(|c| c == '{' || c == '&');
                current = Some(add_schema(engine, artifact, &file, name, &format!("graphql.{declaration}"), index + 1));
                continue;
            }
        }
        if line == "}" { current = None; continue; }
        let Some(schema) = current.as_deref() else { continue; };
        let Some((field, _)) = line.split_once(':') else { continue; };
        let field = field.trim().split('(').next().unwrap_or("");
        if field.is_empty() { continue; }
        let id = format!("{schema}::field:{field}");
        ensure(engine, CausalEntity { id:id.clone(), kind:CausalEntityKind::Value, name:field.to_string(), repository:Some(artifact.repository.clone()), path:Some(artifact.path.clone()), attributes:BTreeMap::new() });
        add(engine, static_fact(schema, &id, CausalRelationKind::Defines, artifact, index + 1));
    }
}

fn extract_proto(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let file = ensure_file(engine, artifact);
    let mut message: Option<String> = None;
    let mut service: Option<String> = None;
    for (index, raw) in artifact.content.lines().enumerate() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("message ") {
            let name = rest.split_whitespace().next().unwrap_or("").trim_matches('{');
            if !name.is_empty() {
                let id = format!("repo:{}::schema:{}", artifact.repository, name);
                ensure(engine, CausalEntity { id:id.clone(), kind:if name.to_ascii_lowercase().ends_with("event") { CausalEntityKind::Event } else { CausalEntityKind::Schema }, name:name.to_string(), repository:Some(artifact.repository.clone()), path:Some(artifact.path.clone()), attributes:BTreeMap::from([("contract.kind".to_string(), "protobuf.message".to_string())]) });
                add(engine, static_fact(&file, &id, CausalRelationKind::Defines, artifact, index + 1));
                message = Some(id); service = None;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("service ") {
            let name = rest.split_whitespace().next().unwrap_or("").trim_matches('{');
            if !name.is_empty() {
                let id = format!("repo:{}::service:{}", artifact.repository, name);
                ensure(engine, CausalEntity { id:id.clone(), kind:CausalEntityKind::Service, name:name.to_string(), repository:Some(artifact.repository.clone()), path:Some(artifact.path.clone()), attributes:BTreeMap::from([("contract.kind".to_string(), "protobuf.service".to_string())]) });
                add(engine, static_fact(&file, &id, CausalRelationKind::Defines, artifact, index + 1));
                service = Some(id); message = None;
            }
            continue;
        }
        if line == "}" { message = None; service = None; continue; }
        if let Some(service_id) = service.as_deref() {
            if let Some(rest) = line.strip_prefix("rpc ") {
                let name = rest.split(|character: char| character == '(' || character.is_whitespace()).next().unwrap_or("");
                if !name.is_empty() {
                    let id = format!("{service_id}::rpc:{name}");
                    ensure(engine, CausalEntity { id:id.clone(), kind:CausalEntityKind::Api, name:name.to_string(), repository:Some(artifact.repository.clone()), path:Some(artifact.path.clone()), attributes:BTreeMap::from([("contract.kind".to_string(), "protobuf.rpc".to_string())]) });
                    add(engine, static_fact(service_id, &id, CausalRelationKind::Defines, artifact, index + 1));
                }
            }
        }
        if let Some(message_id) = message.as_deref() {
            if line.contains('=') && line.ends_with(';') {
                let words: Vec<&str> = line.split_whitespace().collect();
                if words.len() >= 2 {
                    let field = words[1].trim();
                    let id = format!("{message_id}::field:{field}");
                    ensure(engine, CausalEntity { id:id.clone(), kind:CausalEntityKind::Value, name:field.to_string(), repository:Some(artifact.repository.clone()), path:Some(artifact.path.clone()), attributes:BTreeMap::from([("protobuf.type".to_string(), words[0].to_string())]) });
                    add(engine, static_fact(message_id, &id, CausalRelationKind::Defines, artifact, index + 1));
                }
            }
        }
    }
}

fn is_compose(path: &str) -> bool {
    path.ends_with("docker-compose.yml") || path.ends_with("docker-compose.yaml") || path.ends_with("compose.yml") || path.ends_with("compose.yaml")
}

fn extract_compose(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let mut service: Option<String> = None;
    let mut in_depends = false;
    for (index, raw) in artifact.content.lines().enumerate() {
        let indent = raw.chars().take_while(|character| character.is_whitespace()).count();
        let line = raw.trim();
        if indent == 2 && line.ends_with(':') && !matches!(line, "services:" | "volumes:" | "networks:") {
            let name = line.trim_end_matches(':');
            let id = format!("repo:{}::service:{}", artifact.repository, name);
            ensure(engine, CausalEntity { id:id.clone(), kind:CausalEntityKind::Service, name:name.to_string(), repository:Some(artifact.repository.clone()), path:Some(artifact.path.clone()), attributes:BTreeMap::new() });
            service = Some(id); in_depends = false; continue;
        }
        if indent == 4 && line == "depends_on:" { in_depends = true; continue; }
        if indent <= 4 && line != "depends_on:" { in_depends = false; }
        if !in_depends || indent < 6 { continue; }
        let dependency = line.trim_start_matches('-').split(':').next().unwrap_or("").trim();
        if dependency.is_empty() { continue; }
        let Some(source) = service.as_deref() else { continue; };
        let target = format!("repo:{}::service:{}", artifact.repository, dependency);
        ensure(engine, CausalEntity { id:target.clone(), kind:CausalEntityKind::Service, name:dependency.to_string(), repository:Some(artifact.repository.clone()), path:Some(artifact.path.clone()), attributes:BTreeMap::new() });
        add(engine, static_fact(source, &target, CausalRelationKind::DependsOn, artifact, index + 1));
    }
}

fn extract_data_access(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let file = ensure_file(engine, artifact);
    let tables: Vec<(String, String)> = engine.entities()
        .filter(|entity| entity.repository.as_deref() == Some(&artifact.repository) && entity.kind == CausalEntityKind::Table)
        .map(|entity| (entity.name.to_ascii_lowercase(), entity.id.clone()))
        .collect();
    for (index, raw) in artifact.content.lines().enumerate() {
        let lower = raw.to_ascii_lowercase();
        for (keyword, relation) in [
            (" from ", CausalRelationKind::Reads), (" join ", CausalRelationKind::Reads),
            ("insert into ", CausalRelationKind::Writes), ("update ", CausalRelationKind::Writes),
            ("delete from ", CausalRelationKind::Writes),
        ] {
            let Some(position) = lower.find(keyword) else { continue; };
            let after = &raw[position + keyword.len()..];
            let table = after.trim_start().split(|character: char| character.is_whitespace() || matches!(character, ';' | ',' | '(' | ')')).next().unwrap_or("");
            let table = table.trim_matches(|character| matches!(character, '`' | '"' | '[' | ']'));
            if table.is_empty() { continue; }
            let target = tables.iter().find(|(name, _)| name.eq_ignore_ascii_case(table)).map(|(_, id)| id.clone()).unwrap_or_else(|| format!("repo:{}::table:{}", artifact.repository, table));
            ensure(engine, CausalEntity { id:target.clone(), kind:CausalEntityKind::Table, name:table.to_string(), repository:Some(artifact.repository.clone()), path:None, attributes:BTreeMap::from([("discovered.by".to_string(), "sql_identifier".to_string())]) });
            add(engine, static_fact(&file, &target, relation, artifact, index + 1));
        }

        let Some(position) = lower.find("prisma.") else { continue; };
        let rest = &raw[position + "prisma.".len()..];
        let model = rest.split('.').next().unwrap_or("").trim();
        let method = rest.split('.').nth(1).unwrap_or("").split(|character: char| character == '(' || character.is_whitespace()).next().unwrap_or("");
        if model.is_empty() { continue; }
        let Some((_, target)) = tables.iter().find(|(name, _)| name.eq_ignore_ascii_case(model)) else { continue; };
        let write = matches!(method.to_ascii_lowercase().as_str(), "create" | "createmany" | "update" | "updatemany" | "upsert" | "delete" | "deletemany");
        let mut relation = static_fact(&file, target, if write { CausalRelationKind::Writes } else { CausalRelationKind::Reads }, artifact, index + 1);
        relation.metadata.insert("orm".to_string(), "prisma".to_string());
        relation.metadata.insert("orm.model_accessor".to_string(), model.to_string());
        relation.metadata.insert("orm.method".to_string(), method.to_string());
        add(engine, relation);
    }
}

fn extract_config_guards(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let file = ensure_file(engine, artifact);
    for (index, raw) in artifact.content.lines().enumerate() {
        let line = raw.trim();
        let lower = line.to_ascii_lowercase();
        if !(lower.starts_with("if ") || lower.starts_with("if(") || lower.starts_with("if (")) { continue; }
        let keys = env_keys(line);
        if keys.is_empty() { continue; }
        let branch = format!("{file}::branch:{}", index + 1);
        ensure(engine, CausalEntity { id:branch.clone(), kind:CausalEntityKind::Symbol, name:format!("branch line {}", index + 1), repository:Some(artifact.repository.clone()), path:Some(artifact.path.clone()), attributes:BTreeMap::from([("condition".to_string(), line.to_string())]) });
        add(engine, static_fact(&file, &branch, CausalRelationKind::Defines, artifact, index + 1));
        for key in keys {
            let config = format!("repo:{}::config:{}", artifact.repository, key);
            ensure(engine, CausalEntity { id:config.clone(), kind:CausalEntityKind::Configuration, name:key, repository:Some(artifact.repository.clone()), path:Some(artifact.path.clone()), attributes:BTreeMap::new() });
            let mut relation = static_fact(&config, &branch, CausalRelationKind::Guards, artifact, index + 1);
            relation.condition = Some(line.to_string());
            add(engine, relation);
        }
    }
}

fn env_keys(line: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let marker = "process.env.";
    let mut cursor = 0usize;
    while cursor < line.len() {
        let Some(relative) = line[cursor..].find(marker) else { break; };
        let start = cursor + relative + marker.len();
        let key: String = line[start..].chars().take_while(|character| character.is_ascii_alphanumeric() || *character == '_').collect();
        if !key.is_empty() { keys.push(key.clone()); }
        cursor = start.saturating_add(key.len()).max(start + 1);
    }
    for function in ["os.getenv", "env::var", "System.getenv"] {
        let mut cursor = 0usize;
        while cursor < line.len() {
            let Some(relative) = line[cursor..].find(function) else { break; };
            let start = cursor + relative + function.len();
            let tail = &line[start..];
            let quote = if tail.contains('"') { '"' } else if tail.contains('\'') { '\'' } else { cursor = start + 1; continue; };
            let Some(first) = tail.find(quote) else { cursor = start + 1; continue; };
            let remainder = &tail[first + 1..];
            let Some(second) = remainder.find(quote) else { cursor = start + 1; continue; };
            let key = &remainder[..second];
            if !key.is_empty() { keys.push(key.to_string()); }
            cursor = start + first + second + 2;
        }
    }
    keys.sort(); keys.dedup(); keys
}

fn is_test(path: &str) -> bool {
    path.contains("/tests/") || path.starts_with("tests/") || path.contains("/__tests__/") ||
    path.ends_with("_test.rs") || path.ends_with("_test.go") || path.ends_with("_test.py") ||
    path.ends_with(".test.ts") || path.ends_with(".spec.ts") || path.ends_with(".test.tsx") || path.ends_with(".spec.tsx")
}

fn extract_test_imports(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact, paths: Option<&HashSet<String>>) {
    let Some(paths) = paths else { return; };
    let test = ensure_file(engine, artifact);
    for (index, line) in artifact.content.lines().enumerate() {
        for import in relative_quoted_values(line) {
            let Some(target_path) = resolve_import(&artifact.path, &import, paths) else { continue; };
            let target = format!("repo:{}::file:{}", artifact.repository, target_path);
            ensure(engine, CausalEntity { id:target.clone(), kind:CausalEntityKind::File, name:target_path.rsplit('/').next().unwrap_or(&target_path).to_string(), repository:Some(artifact.repository.clone()), path:Some(target_path), attributes:BTreeMap::new() });
            let mut relation = static_fact(&test, &target, CausalRelationKind::Exercises, artifact, index + 1);
            relation.metadata.insert("basis".to_string(), "test_relative_import".to_string());
            add(engine, relation);
        }
    }
}

fn relative_quoted_values(line: &str) -> Vec<String> {
    let mut values = Vec::new();
    for quote in ['"', '\''] {
        let mut rest = line;
        while let Some(start) = rest.find(quote) {
            let tail = &rest[start + 1..];
            let Some(end) = tail.find(quote) else { break; };
            let value = &tail[..end];
            if value.starts_with('.') { values.push(value.to_string()); }
            rest = &tail[end + 1..];
        }
    }
    values.sort(); values.dedup(); values
}

fn resolve_import(from: &str, import: &str, paths: &HashSet<String>) -> Option<String> {
    let parent = Path::new(from).parent().unwrap_or_else(|| Path::new(""));
    let base = lexical(&parent.join(import));
    let candidates = vec![
        base.clone(), format!("{base}.ts"), format!("{base}.tsx"), format!("{base}.js"), format!("{base}.jsx"),
        format!("{base}.py"), format!("{base}.rs"), format!("{base}.go"), format!("{base}/index.ts"),
        format!("{base}/index.tsx"), format!("{base}/index.js"), format!("{base}/mod.rs"),
    ];
    candidates.into_iter().find(|candidate| paths.contains(candidate))
}

fn lexical(path: &Path) -> String {
    let mut stack: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => { stack.pop(); }
            Component::CurDir => {}
            Component::Normal(value) => stack.push(value.to_string_lossy().to_string()),
            Component::RootDir => stack.clear(),
            Component::Prefix(prefix) => stack.push(prefix.as_os_str().to_string_lossy().to_string()),
        }
    }
    stack.join("/")
}

fn normalize(path: &str) -> String { path.replace('\\', "/").trim_start_matches("./").to_string() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn resolves_test_import() {
        let paths = HashSet::from(["src/auth.ts".to_string()]);
        assert_eq!(resolve_import("tests/auth.test.ts", "../src/auth", &paths), Some("src/auth.ts".to_string()));
    }
    #[test] fn extracts_process_env_guard_key() {
        assert_eq!(env_keys("if (process.env.FEATURE_X === 'on')"), vec!["FEATURE_X".to_string()]);
    }
}
