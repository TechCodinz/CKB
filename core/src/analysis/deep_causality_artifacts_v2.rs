//! Higher-fidelity repository artifact extraction for V13.1.
//!
//! This pass complements the conservative baseline extractor with constructs
//! that can be recognized from explicit syntax: OpenAPI paths/methods,
//! GraphQL/protobuf declarations, Docker Compose service dependencies,
//! package-manifest dependencies, SQL/Prisma model access, config-guarded
//! branches, and test imports. It does not infer framework behavior when an
//! identifier cannot be resolved from observed repository evidence.

use super::deep_causality::*;
use super::deep_causality_extractors::RepositoryArtifact;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

pub fn enrich_deep_artifact_semantics(engine: &mut DeepCausalityEngine, artifacts: &[RepositoryArtifact]) {
    let paths_by_repo = artifact_paths(artifacts);
    for artifact in artifacts {
        let lower = artifact.path.to_ascii_lowercase();
        if is_package_manifest(&lower) { extract_package_manifest(engine, artifact); }
        if is_openapi(&lower, &artifact.content) { extract_openapi(engine, artifact); }
        if lower.ends_with(".graphql") || lower.ends_with(".gql") { extract_graphql(engine, artifact); }
        if lower.ends_with(".proto") { extract_proto(engine, artifact); }
        if is_compose(&lower) { extract_compose_dependencies(engine, artifact); }
        extract_sql_and_prisma_access(engine, artifact);
        extract_config_guards(engine, artifact);
        if is_test_path(&lower) {
            extract_test_imports(engine, artifact, paths_by_repo.get(&artifact.repository));
        }
    }
}

fn artifact_paths(artifacts: &[RepositoryArtifact]) -> HashMap<String, HashSet<String>> {
    let mut out: HashMap<String, HashSet<String>> = HashMap::new();
    for artifact in artifacts {
        out.entry(artifact.repository.clone()).or_default().insert(normalize_path(&artifact.path));
    }
    out
}

fn ensure(engine: &mut DeepCausalityEngine, entity: CausalEntity) {
    if !engine.entities().any(|e| e.id == entity.id) { engine.upsert_entity(entity); }
}

fn add_fact(engine: &mut DeepCausalityEngine, mut causal: CausalFact) {
    let duplicate = engine.facts().iter().any(|f|
        f.from == causal.from && f.to == causal.to && f.relation == causal.relation &&
        f.evidence == causal.evidence && f.condition == causal.condition
    );
    if duplicate { return; }
    causal.confidence = causal.confidence.clamp(0.0, 1.0);
    let _ = engine.add_fact(causal);
}

fn static_fact(from: &str, to: &str, relation: CausalRelationKind, artifact: &RepositoryArtifact, line: usize) -> CausalFact {
    CausalFact {
        from: from.into(), to: to.into(), relation,
        evidence: CausalEvidenceClass::Static, confidence: 1.0,
        condition: None, timestamp_ms: None,
        metadata: BTreeMap::from([
            ("source.path".into(), artifact.path.clone()),
            ("source.line".into(), line.to_string()),
        ]),
    }
}

fn file_id(artifact: &RepositoryArtifact) -> String {
    format!("repo:{}::file:{}", artifact.repository, normalize_path(&artifact.path))
}

fn is_package_manifest(path: &str) -> bool {
    path.ends_with("package.json") || path.ends_with("cargo.toml") || path.ends_with("go.mod") || path.ends_with("pyproject.toml")
}

fn extract_package_manifest(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let file = file_id(artifact);
    ensure(engine, CausalEntity { id:file.clone(), kind:CausalEntityKind::File, name:artifact.path.clone(), repository:Some(artifact.repository.clone()), path:Some(normalize_path(&artifact.path)), attributes:BTreeMap::new() });
    let lower = artifact.path.to_ascii_lowercase();
    if lower.ends_with("package.json") {
        if let Ok(json) = serde_json::from_str::<Value>(&artifact.content) {
            if let Some(name) = json.get("name").and_then(Value::as_str) {
                let package = repo_package_id(&artifact.repository, name);
                ensure_package(engine, &package, name, Some(&artifact.repository));
                add_fact(engine, static_fact(&file, &package, CausalRelationKind::Defines, artifact, 1));
                for section in ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"] {
                    if let Some(map) = json.get(section).and_then(Value::as_object) {
                        for dep in map.keys() {
                            let external = external_package_id(dep);
                            ensure_package(engine, &external, dep, None);
                            let mut fact = static_fact(&package, &external, CausalRelationKind::DependsOn, artifact, 1);
                            fact.metadata.insert("manifest.section".into(), section.into());
                            add_fact(engine, fact);
                        }
                    }
                }
            }
        }
    } else if lower.ends_with("cargo.toml") {
        let mut section = "";
        let mut package_name: Option<String> = None;
        for (idx, raw) in artifact.content.lines().enumerate() {
            let line = raw.trim();
            if line.starts_with('[') && line.ends_with(']') { section = line.trim_matches(&['[',']'][..]); continue; }
            if section == "package" && line.starts_with("name") {
                if let Some((_, value)) = line.split_once('=') { package_name = Some(value.trim().trim_matches('"').to_string()); }
            }
            if matches!(section, "dependencies" | "dev-dependencies" | "build-dependencies") {
                if let Some((name, _)) = line.split_once('=') {
                    let dep = name.trim();
                    if !dep.is_empty() && !dep.starts_with('#') {
                        if let Some(pkg) = package_name.as_ref() {
                            let source = repo_package_id(&artifact.repository, pkg);
                            ensure_package(engine, &source, pkg, Some(&artifact.repository));
                            let target = external_package_id(dep);
                            ensure_package(engine, &target, dep, None);
                            let mut f = static_fact(&source, &target, CausalRelationKind::DependsOn, artifact, idx+1);
                            f.metadata.insert("manifest.section".into(), section.into());
                            add_fact(engine, f);
                        }
                    }
                }
            }
        }
        if let Some(name) = package_name {
            let package = repo_package_id(&artifact.repository, &name);
            ensure_package(engine, &package, &name, Some(&artifact.repository));
            add_fact(engine, static_fact(&file, &package, CausalRelationKind::Defines, artifact, 1));
        }
    } else if lower.ends_with("go.mod") {
        let mut module: Option<String> = None;
        for (idx, raw) in artifact.content.lines().enumerate() {
            let line = raw.trim();
            if let Some(name) = line.strip_prefix("module ") {
                module = Some(name.trim().into());
                let package = repo_package_id(&artifact.repository, name.trim());
                ensure_package(engine, &package, name.trim(), Some(&artifact.repository));
                add_fact(engine, static_fact(&file, &package, CausalRelationKind::Defines, artifact, idx+1));
            } else if let Some(dep) = line.strip_prefix("require ").and_then(|r| r.split_whitespace().next()) {
                if let Some(source_name) = module.as_ref() {
                    let source = repo_package_id(&artifact.repository, source_name);
                    let target = external_package_id(dep);
                    ensure_package(engine, &target, dep, None);
                    add_fact(engine, static_fact(&source, &target, CausalRelationKind::DependsOn, artifact, idx+1));
                }
            }
        }
    } else if lower.ends_with("pyproject.toml") {
        let mut in_project = false;
        for (idx, raw) in artifact.content.lines().enumerate() {
            let line = raw.trim();
            if line.starts_with('[') { in_project = matches!(line, "[project]" | "[tool.poetry]"); continue; }
            if in_project && line.starts_with("name") {
                if let Some((_, value)) = line.split_once('=') {
                    let name = value.trim().trim_matches(['"','\'']);
                    if !name.is_empty() {
                        let package = repo_package_id(&artifact.repository, name);
                        ensure_package(engine, &package, name, Some(&artifact.repository));
                        add_fact(engine, static_fact(&file, &package, CausalRelationKind::Defines, artifact, idx+1));
                    }
                }
            }
        }
    }
}

fn ensure_package(engine: &mut DeepCausalityEngine, id: &str, name: &str, repository: Option<&str>) {
    ensure(engine, CausalEntity { id:id.into(), kind:CausalEntityKind::Package, name:name.into(), repository:repository.map(str::to_string), path:None, attributes:BTreeMap::from([("package.name".into(), name.into())]) });
}
fn repo_package_id(repo: &str, name: &str) -> String { format!("repo:{repo}::package:{name}") }
fn external_package_id(name: &str) -> String { format!("package:external:{name}") }

fn is_openapi(path: &str, content: &str) -> bool {
    path.contains("openapi") || path.contains("swagger") || content.lines().take(8).any(|l| {
        let t=l.trim(); t.starts_with("openapi:") || t.starts_with("swagger:") || t.contains("\"openapi\"")
    })
}

fn extract_openapi(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let file = file_id(artifact);
    if artifact.path.to_ascii_lowercase().ends_with(".json") {
        if let Ok(json) = serde_json::from_str::<Value>(&artifact.content) {
            if let Some(paths) = json.get("paths").and_then(Value::as_object) {
                for (path, item) in paths {
                    if let Some(methods) = item.as_object() {
                        for method in methods.keys().filter(|m| is_http_method(m)) {
                            add_api(engine, artifact, &file, method, path, 1);
                        }
                    }
                }
            }
            if let Some(schemas) = json.pointer("/components/schemas").and_then(Value::as_object) {
                for name in schemas.keys() { add_contract_schema(engine, artifact, &file, name, 1); }
            }
            return;
        }
    }
    let mut current_path: Option<String> = None;
    let mut under_paths = false;
    let mut under_schemas = false;
    for (idx, raw) in artifact.content.lines().enumerate() {
        let indent = raw.chars().take_while(|c| c.is_whitespace()).count();
        let line = raw.trim();
        if line == "paths:" { under_paths=true; under_schemas=false; continue; }
        if line == "schemas:" && indent >= 2 { under_schemas=true; under_paths=false; continue; }
        if under_paths && indent == 2 && line.starts_with('/') && line.ends_with(':') {
            current_path = Some(line.trim_end_matches(':').to_string());
            continue;
        }
        if under_paths && indent == 4 && line.ends_with(':') {
            let method = line.trim_end_matches(':').to_ascii_lowercase();
            if is_http_method(&method) {
                if let Some(path) = current_path.as_ref() { add_api(engine, artifact, &file, &method, path, idx+1); }
            }
        }
        if under_schemas && indent >= 4 && line.ends_with(':') && !line.starts_with('$') {
            let name = line.trim_end_matches(':');
            if !matches!(name, "type"|"properties"|"required"|"items"|"description") {
                add_contract_schema(engine, artifact, &file, name, idx+1);
            }
        }
    }
}

fn is_http_method(method: &str) -> bool { matches!(method.to_ascii_lowercase().as_str(), "get"|"post"|"put"|"patch"|"delete"|"options"|"head"|"trace") }
fn add_api(engine:&mut DeepCausalityEngine, artifact:&RepositoryArtifact, file:&str, method:&str, path:&str, line:usize) {
    let id=format!("repo:{}::api:{}:{}",artifact.repository,method.to_ascii_uppercase(),path);
    ensure(engine,CausalEntity{id:id.clone(),kind:CausalEntityKind::Api,name:format!("{} {}",method.to_ascii_uppercase(),path),repository:Some(artifact.repository.clone()),path:Some(artifact.path.clone()),attributes:BTreeMap::from([("http.method".into(),method.to_ascii_uppercase()),("http.path".into(),path.into())])});
    add_fact(engine,static_fact(file,&id,CausalRelationKind::Defines,artifact,line));
}
fn add_contract_schema(engine:&mut DeepCausalityEngine, artifact:&RepositoryArtifact, file:&str, name:&str, line:usize) {
    let id=format!("repo:{}::schema:{}",artifact.repository,name);
    ensure(engine,CausalEntity{id:id.clone(),kind:CausalEntityKind::Schema,name:name.into(),repository:Some(artifact.repository.clone()),path:Some(artifact.path.clone()),attributes:BTreeMap::new()});
    add_fact(engine,static_fact(file,&id,CausalRelationKind::Defines,artifact,line));
}

fn extract_graphql(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
    let file=file_id(artifact);
    let mut current:Option<String>=None;
    for (idx,raw) in artifact.content.lines().enumerate(){
        let line=raw.trim();
        let mut parts=line.split_whitespace();
        if let Some(kind)=parts.next(){
            if matches!(kind,"type"|"input"|"interface"|"enum"|"union") {
                if let Some(name)=parts.next(){
                    let name=name.trim_matches(|c:char| c=='{' || c=='&');
                    let id=format!("repo:{}::schema:{}",artifact.repository,name);
                    ensure(engine,CausalEntity{id:id.clone(),kind:CausalEntityKind::Schema,name:name.into(),repository:Some(artifact.repository.clone()),path:Some(artifact.path.clone()),attributes:BTreeMap::from([("contract.kind".into(),format!("graphql.{kind}"))])});
                    add_fact(engine,static_fact(&file,&id,CausalRelationKind::Defines,artifact,idx+1)); current=Some(id); continue;
                }
            }
        }
        if line=="}" { current=None; continue; }
        if let Some(schema)=current.as_ref(){
            if let Some((field,_))=line.split_once(':'){
                let field=field.trim().split('(').next().unwrap_or("");
                if !field.is_empty(){
                    let id=format!("{}::field:{}",schema,field);
                    ensure(engine,CausalEntity{id:id.clone(),kind:CausalEntityKind::Value,name:field.into(),repository:Some(artifact.repository.clone()),path:Some(artifact.path.clone()),attributes:BTreeMap::new()});
                    add_fact(engine,static_fact(schema,&id,CausalRelationKind::Defines,artifact,idx+1));
                }
            }
        }
    }
}

fn extract_proto(engine:&mut DeepCausalityEngine,artifact:&RepositoryArtifact){
    let file=file_id(artifact); let mut current_message:Option<String>=None; let mut current_service:Option<String>=None;
    for (idx,raw) in artifact.content.lines().enumerate(){ let line=raw.trim();
        if let Some(rest)=line.strip_prefix("message "){ let name=rest.split_whitespace().next().unwrap_or("").trim_matches('{'); if !name.is_empty(){ let id=format!("repo:{}::schema:{}",artifact.repository,name); ensure(engine,CausalEntity{id:id.clone(),kind:if name.to_ascii_lowercase().ends_with("event"){CausalEntityKind::Event}else{CausalEntityKind::Schema},name:name.into(),repository:Some(artifact.repository.clone()),path:Some(artifact.path.clone()),attributes:BTreeMap::from([("contract.kind".into(),"protobuf.message".into())])}); add_fact(engine,static_fact(&file,&id,CausalRelationKind::Defines,artifact,idx+1)); current_message=Some(id); current_service=None;} continue; }
        if let Some(rest)=line.strip_prefix("service "){ let name=rest.split_whitespace().next().unwrap_or("").trim_matches('{'); if !name.is_empty(){ let id=format!("repo:{}::service:{}",artifact.repository,name); ensure(engine,CausalEntity{id:id.clone(),kind:CausalEntityKind::Service,name:name.into(),repository:Some(artifact.repository.clone()),path:Some(artifact.path.clone()),attributes:BTreeMap::from([("contract.kind".into(),"protobuf.service".into())])}); add_fact(engine,static_fact(&file,&id,CausalRelationKind::Defines,artifact,idx+1)); current_service=Some(id); current_message=None;} continue; }
        if line=="}"{current_message=None; current_service=None; continue;}
        if let Some(service)=current_service.as_ref(){ if let Some(rest)=line.strip_prefix("rpc "){ let name=rest.split(|c:char| c=='(' || c.is_whitespace()).next().unwrap_or(""); if !name.is_empty(){ let id=format!("{}::rpc:{}",service,name); ensure(engine,CausalEntity{id:id.clone(),kind:CausalEntityKind::Api,name:name.into(),repository:Some(artifact.repository.clone()),path:Some(artifact.path.clone()),attributes:BTreeMap::from([("contract.kind".into(),"protobuf.rpc".into())])}); add_fact(engine,static_fact(service,&id,CausalRelationKind::Defines,artifact,idx+1)); } } }
        if let Some(message)=current_message.as_ref(){ if line.contains('=') && line.ends_with(';'){ let tokens:Vec<_>=line.split_whitespace().collect(); if tokens.len()>=2{ let name=tokens[1].trim(); let id=format!("{}::field:{}",message,name); ensure(engine,CausalEntity{id:id.clone(),kind:CausalEntityKind::Value,name:name.into(),repository:Some(artifact.repository.clone()),path:Some(artifact.path.clone()),attributes:BTreeMap::from([("protobuf.type".into(),tokens[0].into())])}); add_fact(engine,static_fact(message,&id,CausalRelationKind::Defines,artifact,idx+1)); } } }
    }
}

fn is_compose(path:&str)->bool{path.ends_with("docker-compose.yml")||path.ends_with("docker-compose.yaml")||path.ends_with("compose.yml")||path.ends_with("compose.yaml")}
fn extract_compose_dependencies(engine:&mut DeepCausalityEngine,artifact:&RepositoryArtifact){
    let mut current_service:Option<String>=None; let mut in_depends=false;
    for (idx,raw) in artifact.content.lines().enumerate(){ let indent=raw.chars().take_while(|c|c.is_whitespace()).count(); let line=raw.trim();
        if indent==2 && line.ends_with(':') && !matches!(line,"services:"|"volumes:"|"networks:"){ let name=line.trim_end_matches(':'); let id=format!("repo:{}::service:{}",artifact.repository,name); ensure(engine,CausalEntity{id:id.clone(),kind:CausalEntityKind::Service,name:name.into(),repository:Some(artifact.repository.clone()),path:Some(artifact.path.clone()),attributes:BTreeMap::new()}); current_service=Some(id); in_depends=false; continue; }
        if indent==4 && line=="depends_on:"{in_depends=true; continue;}
        if indent<=4 && line!="depends_on:"{in_depends=false;}
        if in_depends && indent>=6 { let dep=line.trim_start_matches('-').split(':').next().unwrap_or("").trim(); if dep.is_empty(){continue;} if let Some(source)=current_service.as_ref(){ let target=format!("repo:{}::service:{}",artifact.repository,dep); ensure(engine,CausalEntity{id:target.clone(),kind:CausalEntityKind::Service,name:dep.into(),repository:Some(artifact.repository.clone()),path:Some(artifact.path.clone()),attributes:BTreeMap::new()}); add_fact(engine,static_fact(source,&target,CausalRelationKind::DependsOn,artifact,idx+1)); } }
    }
}

fn extract_sql_and_prisma_access(engine:&mut DeepCausalityEngine,artifact:&RepositoryArtifact){
    let file=file_id(artifact); let tables:Vec<(String,String)>=engine.entities().filter(|e|e.repository.as_deref()==Some(&artifact.repository)&&e.kind==CausalEntityKind::Table).map(|e|(e.name.to_ascii_lowercase(),e.id.clone())).collect();
    for (idx,raw) in artifact.content.lines().enumerate(){ let lower=raw.to_ascii_lowercase();
        for (keyword,relation) in [(" from ",CausalRelationKind::Reads),(" join ",CausalRelationKind::Reads),("insert into ",CausalRelationKind::Writes),("update ",CausalRelationKind::Writes),("delete from ",CausalRelationKind::Writes)] {
            if let Some(pos)=lower.find(keyword){ let after=&raw[pos+keyword.len()..]; let table=after.trim_start().split(|c:char| c.is_whitespace()||matches!(c,';'|','|'('|')')).next().unwrap_or("").trim_matches(['`','"','[',']']); if !table.is_empty(){ let target=tables.iter().find(|(name,_)|name.eq_ignore_ascii_case(table)).map(|(_,id)|id.clone()).unwrap_or_else(||format!("repo:{}::table:{}",artifact.repository,table)); ensure(engine,CausalEntity{id:target.clone(),kind:CausalEntityKind::Table,name:table.into(),repository:Some(artifact.repository.clone()),path:None,attributes:BTreeMap::from([("discovered.by".into(),"sql_identifier".into())])}); add_fact(engine,static_fact(&file,&target,relation.clone(),artifact,idx+1)); } }
        }
        if let Some(pos)=lower.find("prisma."){ let rest=&raw[pos+7..]; let model=rest.split('.').next().unwrap_or("").trim(); let method=rest.split('.').nth(1).unwrap_or("").split(|c:char|c=='('||c.is_whitespace()).next().unwrap_or(""); if !model.is_empty(){ if let Some((_,target))=tables.iter().find(|(name,_)|name.eq_ignore_ascii_case(model)){ let relation=if matches!(method,"create"|"createMany"|"update"|"updateMany"|"upsert"|"delete"|"deleteMany"){CausalRelationKind::Writes}else{CausalRelationKind::Reads}; let mut f=static_fact(&file,target,relation,artifact,idx+1); f.metadata.insert("orm".into(),"prisma".into()); f.metadata.insert("orm.model_accessor".into(),model.into()); f.metadata.insert("orm.method".into(),method.into()); add_fact(engine,f); } } }
    }
}

fn extract_config_guards(engine:&mut DeepCausalityEngine,artifact:&RepositoryArtifact){ let file=file_id(artifact);
    for (idx,raw) in artifact.content.lines().enumerate(){ let line=raw.trim(); let lower=line.to_ascii_lowercase(); let condition=if lower.starts_with("if ")||lower.starts_with("if(")||lower.starts_with("if ("){Some(line.to_string())}else{None}; let Some(condition)=condition else{continue;};
        let branch=format!("{}::branch:{}",file,idx+1); ensure(engine,CausalEntity{id:branch.clone(),kind:CausalEntityKind::Symbol,name:format!("branch line {}",idx+1),repository:Some(artifact.repository.clone()),path:Some(artifact.path.clone()),attributes:BTreeMap::from([("condition".into(),condition.clone())])}); add_fact(engine,static_fact(&file,&branch,CausalRelationKind::Defines,artifact,idx+1));
        for key in explicit_env_keys(line){ let config=format!("repo:{}::config:{}",artifact.repository,key); ensure(engine,CausalEntity{id:config.clone(),kind:CausalEntityKind::Configuration,name:key.clone(),repository:Some(artifact.repository.clone()),path:Some(artifact.path.clone()),attributes:BTreeMap::new()}); let mut f=static_fact(&config,&branch,CausalRelationKind::Guards,artifact,idx+1); f.condition=Some(condition.clone()); add_fact(engine,f); }
    }
}

fn explicit_env_keys(line:&str)->Vec<String>{ let mut out=Vec::new(); for needle in ["process.env.","os.getenv(\"","os.getenv('","env::var(\"","System.getenv(\""]{ let mut start=0; while let Some(pos)=line[start..].find(needle){ let begin=start+pos+needle.len(); let tail=&line[begin..]; let key=if needle=="process.env."{tail.chars().take_while(|c|c.is_ascii_alphanumeric()||*c=='_').collect::<String>()}else{tail.split(|c|c=='\"'||c=='\'').next().unwrap_or("").to_string()}; if !key.is_empty(){out.push(key);} start=begin+key.len(); if start>=line.len(){break;} } } out.sort(); out.dedup(); out }

fn is_test_path(path:&str)->bool{path.contains("/tests/")||path.contains("/__tests__/")||path.ends_with("_test.rs")||path.ends_with("_test.go")||path.ends_with(".test.ts")||path.ends_with(".spec.ts")||path.ends_with(".test.tsx")||path.ends_with(".spec.tsx")||path.ends_with("_test.py")||path.starts_with("tests/")}
fn extract_test_imports(engine:&mut DeepCausalityEngine,artifact:&RepositoryArtifact,paths:Option<&HashSet<String>>){ let Some(paths)=paths else{return;}; let test=file_id(artifact);
    for (idx,line) in artifact.content.lines().enumerate(){ for spec in quoted_relative_specs(line){ if let Some(target_path)=resolve_import(&artifact.path,&spec,paths){ let target=format!("repo:{}::file:{}",artifact.repository,target_path); ensure(engine,CausalEntity{id:target.clone(),kind:CausalEntityKind::File,name:target_path.rsplit('/').next().unwrap_or(&target_path).into(),repository:Some(artifact.repository.clone()),path:Some(target_path.clone()),attributes:BTreeMap::new()}); let mut f=static_fact(&test,&target,CausalRelationKind::Exercises,artifact,idx+1); f.metadata.insert("basis".into(),"test_relative_import".into()); add_fact(engine,f); } } }
}
fn quoted_relative_specs(line:&str)->Vec<String>{ let mut out=Vec::new(); for quote in ['\"','\'']{ let mut rest=line; while let Some(start)=rest.find(quote){ let tail=&rest[start+1..]; let Some(end)=tail.find(quote) else{break;}; let value=&tail[..end]; if value.starts_with('.') {out.push(value.into());} rest=&tail[end+1..]; } } out.sort(); out.dedup(); out }
fn resolve_import(from:&str,spec:&str,paths:&HashSet<String>)->Option<String>{ let parent=Path::new(from).parent().unwrap_or_else(||Path::new("")); let base=normalize_lexical(&parent.join(spec)); let candidates=[base.clone(),format!("{base}.ts"),format!("{base}.tsx"),format!("{base}.js"),format!("{base}.jsx"),format!("{base}.py"),format!("{base}.rs"),format!("{base}.go"),format!("{base}/index.ts"),format!("{base}/index.tsx"),format!("{base}/index.js"),format!("{base}/mod.rs")]; candidates.into_iter().find(|c|paths.contains(c)) }
fn normalize_lexical(path:&Path)->String{ let mut stack:Vec<String>=Vec::new(); for c in path.components(){ match c{Component::ParentDir=>{stack.pop();},Component::CurDir=>{},Component::Normal(v)=>stack.push(v.to_string_lossy().to_string()),Component::RootDir=>stack.clear(),Component::Prefix(p)=>stack.push(p.as_os_str().to_string_lossy().to_string())} } stack.join("/") }
fn normalize_path(path:&str)->String{path.replace('\\',"/").trim_start_matches("./").to_string()}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn resolves_test_import_candidates(){ let paths=HashSet::from(["src/auth.ts".to_string()]); assert_eq!(resolve_import("tests/auth.test.ts","../src/auth",&paths),Some("src/auth.ts".into())); }
    #[test] fn extracts_env_keys_from_condition(){ assert_eq!(explicit_env_keys("if (process.env.FEATURE_X === 'on')"),vec!["FEATURE_X"]); }
}
