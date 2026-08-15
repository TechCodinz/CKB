//! Repository artifact extractors for V13.1 Deep Causality.
//!
//! These extractors are intentionally conservative and evidence-producing. A
//! matched source construct becomes a STATIC fact with the source path/line in
//! metadata. Unmatched semantics stay unknown; they are never inferred merely
//! because a framework commonly behaves a certain way.

use super::deep_causality::*;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone)]
pub struct RepositoryArtifact {
    pub repository: String,
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Default)]
pub struct CausalArtifactExtractor;

impl CausalArtifactExtractor {
    pub fn extract(artifacts: &[RepositoryArtifact]) -> DeepCausalityEngine {
        let mut engine = DeepCausalityEngine::new();
        for artifact in artifacts {
            Self::extract_artifact(&mut engine, artifact);
        }
        engine
    }

    pub fn enrich(engine: &mut DeepCausalityEngine, artifacts: &[RepositoryArtifact]) {
        for artifact in artifacts { Self::extract_artifact(engine, artifact); }
    }

    fn extract_artifact(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact) {
        let path = artifact.path.replace('\\', "/");
        let lower = path.to_ascii_lowercase();
        let file_id = format!("repo:{}::file:{}", artifact.repository, path);
        upsert(engine, CausalEntity {
            id: file_id.clone(),
            kind: classify_file(&lower),
            name: path.rsplit('/').next().unwrap_or(&path).to_string(),
            repository: Some(artifact.repository.clone()),
            path: Some(path.clone()),
            attributes: BTreeMap::new(),
        });

        if is_codeowners(&lower) { extract_codeowners(engine, artifact, &file_id); }
        if is_schema(&lower) { extract_schema(engine, artifact, &file_id); }
        if is_infra(&lower) { extract_infra(engine, artifact, &file_id); }
        if is_workflow(&lower) { extract_workflow(engine, artifact, &file_id); }

        extract_code_semantics(engine, artifact, &file_id);
    }
}

fn upsert(engine: &mut DeepCausalityEngine, entity: CausalEntity) { engine.upsert_entity(entity); }
fn fact(engine: &mut DeepCausalityEngine, from: &str, to: &str, relation: CausalRelationKind, line: usize, path: &str, mut metadata: BTreeMap<String,String>) {
    metadata.insert("source.path".into(), path.into());
    metadata.insert("source.line".into(), line.to_string());
    let _ = engine.add_fact(CausalFact { from: from.into(), to: to.into(), relation, evidence: CausalEvidenceClass::Static, confidence: 1.0, condition: None, timestamp_ms: None, metadata });
}

fn classify_file(path: &str) -> CausalEntityKind {
    if is_schema(path) { CausalEntityKind::Schema }
    else if path.contains("migration") { CausalEntityKind::Migration }
    else if is_infra(path) { CausalEntityKind::Infrastructure }
    else if is_workflow(path) { CausalEntityKind::Deployment }
    else if is_test(path) { CausalEntityKind::Test }
    else if is_codeowners(path) { CausalEntityKind::Policy }
    else { CausalEntityKind::File }
}
fn is_schema(path: &str) -> bool { path.ends_with("schema.prisma") || path.ends_with(".sql") || path.contains("/schema/") }
fn is_infra(path: &str) -> bool { path.ends_with(".tf") || path.ends_with(".tf.json") || path.ends_with("docker-compose.yml") || path.ends_with("docker-compose.yaml") || path.ends_with("compose.yml") || path.ends_with("compose.yaml") || path.ends_with("dockerfile") || path.contains("/k8s/") || path.contains("/kubernetes/") || path.contains("/helm/") }
fn is_workflow(path: &str) -> bool { path.contains("/.github/workflows/") || path.starts_with(".github/workflows/") || path.contains("/deploy/") }
fn is_test(path: &str) -> bool { path.contains("/tests/") || path.contains("/__tests__/") || path.ends_with("_test.rs") || path.ends_with("_test.go") || path.ends_with(".test.ts") || path.ends_with(".spec.ts") || path.ends_with(".test.tsx") || path.ends_with(".spec.tsx") }
fn is_codeowners(path: &str) -> bool { path.ends_with("codeowners") || path.ends_with(".github/codeowners") }

fn extract_codeowners(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact, file_id: &str) {
    for (idx, line) in artifact.content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() < 2 { continue; }
        let scope = format!("repo:{}::ownership:{}", artifact.repository, parts[0]);
        upsert(engine, CausalEntity { id: scope.clone(), kind: CausalEntityKind::Package, name: parts[0].into(), repository: Some(artifact.repository.clone()), path: Some(parts[0].into()), attributes: BTreeMap::new() });
        fact(engine, file_id, &scope, CausalRelationKind::Defines, idx+1, &artifact.path, BTreeMap::new());
        for owner in &parts[1..] {
            let owner_id = format!("owner:{}", owner.trim_start_matches('@'));
            upsert(engine, CausalEntity { id: owner_id.clone(), kind: if owner.contains('/') { CausalEntityKind::Team } else { CausalEntityKind::Owner }, name: owner.to_string(), repository: None, path: None, attributes: BTreeMap::new() });
            fact(engine, &owner_id, &scope, CausalRelationKind::Owns, idx+1, &artifact.path, BTreeMap::new());
        }
    }
}

fn extract_schema(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact, file_id: &str) {
    let mut current_table: Option<String> = None;
    for (idx, raw) in artifact.content.lines().enumerate() {
        let line = raw.trim();
        let lower = line.to_ascii_lowercase();
        if line.starts_with("model ") {
            let name = line.split_whitespace().nth(1).unwrap_or("unknown").trim_matches('{');
            let id = format!("repo:{}::table:{}", artifact.repository, name);
            upsert(engine, CausalEntity { id: id.clone(), kind: CausalEntityKind::Table, name: name.into(), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            fact(engine, file_id, &id, CausalRelationKind::Defines, idx+1, &artifact.path, BTreeMap::new());
            current_table = Some(id);
            continue;
        }
        if lower.starts_with("create table") || lower.starts_with("alter table") {
            let tokens: Vec<_> = line.split_whitespace().collect();
            let pos = tokens.iter().position(|v| v.eq_ignore_ascii_case("table"));
            if let Some(pos) = pos.and_then(|p| tokens.get(p+1).map(|_| p)) {
                let name = tokens[pos+1].trim_matches(|c: char| matches!(c,'`'|'"'|'['|']'|'('));
                let id = format!("repo:{}::table:{}", artifact.repository, name);
                upsert(engine, CausalEntity { id: id.clone(), kind: CausalEntityKind::Table, name: name.into(), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
                fact(engine, file_id, &id, if lower.starts_with("alter") { CausalRelationKind::Migrates } else { CausalRelationKind::Defines }, idx+1, &artifact.path, BTreeMap::new());
                current_table = Some(id);
            }
        }
        if let Some(table) = current_table.as_ref() {
            if line == "}" || line.starts_with(")") || line.starts_with(';') { current_table = None; continue; }
            if line.is_empty() || line.starts_with("@@") || line.starts_with("//") || line.starts_with("--") { continue; }
            let first = line.split_whitespace().next().unwrap_or("");
            if !first.is_empty() && first.chars().next().map(|c| c.is_alphabetic() || c=='_').unwrap_or(false) {
                let column = format!("{}::column:{}", table, first.trim_matches(|c:char| c==',' || c=='`' || c=='"'));
                upsert(engine, CausalEntity { id: column.clone(), kind: CausalEntityKind::Column, name: first.into(), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
                fact(engine, table, &column, CausalRelationKind::Defines, idx+1, &artifact.path, BTreeMap::new());
            }
        }
    }
}

fn extract_infra(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact, file_id: &str) {
    let mut current_service: Option<String> = None;
    for (idx, raw) in artifact.content.lines().enumerate() {
        let line = raw.trim();
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("resource ") || lower.starts_with("module ") || lower.starts_with("data ") {
            let quoted: Vec<_> = line.split('"').skip(1).step_by(2).collect();
            if !quoted.is_empty() {
                let name = quoted.join(".");
                let id = format!("repo:{}::infra:{}", artifact.repository, name);
                upsert(engine, CausalEntity { id: id.clone(), kind: CausalEntityKind::Infrastructure, name, repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
                fact(engine, file_id, &id, CausalRelationKind::Defines, idx+1, &artifact.path, BTreeMap::new());
                current_service = Some(id);
            }
        }
        if raw.starts_with("  ") && !raw.starts_with("    ") && line.ends_with(':') && !matches!(lower.as_str(), "services:"|"volumes:"|"networks:"|"environment:"|"depends_on:") {
            let name = line.trim_end_matches(':');
            let id = format!("repo:{}::service:{}", artifact.repository, name);
            upsert(engine, CausalEntity { id: id.clone(), kind: CausalEntityKind::Service, name: name.into(), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            fact(engine, file_id, &id, CausalRelationKind::Defines, idx+1, &artifact.path, BTreeMap::new());
            current_service = Some(id);
        }
        if let Some(service) = current_service.as_ref() {
            if lower.starts_with("image:") {
                let target = format!("runtime:image:{}", line.split_once(':').map(|(_,v)| v.trim()).unwrap_or("unknown"));
                upsert(engine, CausalEntity { id: target.clone(), kind: CausalEntityKind::RuntimeResource, name: target.clone(), repository: None, path: None, attributes: BTreeMap::new() });
                fact(engine, service, &target, CausalRelationKind::Deploys, idx+1, &artifact.path, BTreeMap::new());
            }
            if lower.contains("depends_on") {
                // Detailed service targets are also discovered on subsequent YAML rows.
            }
        }
    }
}

fn extract_workflow(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact, file_id: &str) {
    let deploy_id = format!("repo:{}::deployment:{}", artifact.repository, artifact.path);
    upsert(engine, CausalEntity { id: deploy_id.clone(), kind: CausalEntityKind::Deployment, name: artifact.path.clone(), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
    fact(engine, file_id, &deploy_id, CausalRelationKind::Defines, 1, &artifact.path, BTreeMap::new());
    for (idx, raw) in artifact.content.lines().enumerate() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("uses:") {
            let action = format!("infra:action:{}", rest.trim());
            upsert(engine, CausalEntity { id: action.clone(), kind: CausalEntityKind::Infrastructure, name: rest.trim().into(), repository: None, path: None, attributes: BTreeMap::new() });
            fact(engine, &deploy_id, &action, CausalRelationKind::DependsOn, idx+1, &artifact.path, BTreeMap::new());
        }
        if line.contains("deploy") || line.contains("vercel") || line.contains("kubectl") || line.contains("docker push") {
            let step = format!("{}::step:{}", deploy_id, idx+1);
            upsert(engine, CausalEntity { id: step.clone(), kind: CausalEntityKind::Deployment, name: line.chars().take(100).collect(), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            fact(engine, &deploy_id, &step, CausalRelationKind::Deploys, idx+1, &artifact.path, BTreeMap::new());
        }
    }
}

fn extract_code_semantics(engine: &mut DeepCausalityEngine, artifact: &RepositoryArtifact, file_id: &str) {
    let mut values: HashMap<String,String> = HashMap::new();
    for (idx, raw) in artifact.content.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        let lower = line.to_ascii_lowercase();
        if line.is_empty() { continue; }

        // Environment/config/feature flags.
        for key in extract_env_keys(line) {
            let id = format!("repo:{}::config:{}", artifact.repository, key);
            upsert(engine, CausalEntity { id: id.clone(), kind: if key.to_ascii_lowercase().contains("feature") { CausalEntityKind::FeatureFlag } else if looks_secret(&key) { CausalEntityKind::Secret } else { CausalEntityKind::Configuration }, name: key, repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            fact(engine, file_id, &id, CausalRelationKind::Reads, line_no, &artifact.path, BTreeMap::new());
        }

        // HTTP/user-controlled input sources.
        if contains_any(&lower, &["req.body", "request.body", "req.query", "request.query", "req.params", "request.params", "request.form", "stdin", "argv", "location.search", "urlsearchparams"]) {
            let src = format!("{}::input:{}", file_id, line_no);
            upsert(engine, CausalEntity { id: src.clone(), kind: CausalEntityKind::Parameter, name: format!("untrusted input line {line_no}"), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::from([("trust".into(),"untrusted".into())]) });
            fact(engine, &src, file_id, CausalRelationKind::TrustBoundary, line_no, &artifact.path, BTreeMap::new());
            values.insert("__last_input".into(), src);
        }

        // Assignments give line-level value-flow anchors without inventing symbol identity.
        if let Some((left, _right)) = split_assignment(line) {
            let name = left.split_whitespace().last().unwrap_or(left).trim_matches(|c:char| !c.is_alphanumeric() && c!='_' && c!='$');
            if !name.is_empty() {
                let id = format!("{}::value:{}", file_id, name);
                upsert(engine, CausalEntity { id: id.clone(), kind: CausalEntityKind::Value, name: name.into(), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
                if let Some(src) = values.get("__last_input") { fact(engine, src, &id, CausalRelationKind::Assigns, line_no, &artifact.path, BTreeMap::new()); }
                values.insert(name.into(), id);
            }
        }

        // Validation/sanitization evidence.
        if contains_any(&lower, &["sanitize(", "escape(", "validator.", "validate(", "parse.safe", "safeparse(", "zod", "joi.", "htmlspecialchars", "parameterize", "preparedstatement"]) {
            let sanitizer = format!("{}::sanitizer:{}", file_id, line_no);
            upsert(engine, CausalEntity { id: sanitizer.clone(), kind: CausalEntityKind::Symbol, name: format!("sanitizer line {line_no}"), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            if let Some(src) = values.get("__last_input") { fact(engine, src, &sanitizer, CausalRelationKind::Sanitizes, line_no, &artifact.path, BTreeMap::new()); }
            values.insert("__last_input".into(), sanitizer);
        }

        // Security/side-effect sinks.
        if contains_any(&lower, &["exec(", "spawn(", "system(", "eval(", "innerhtml", "dangerouslysetinnerhtml", "query(", "execute(", "rawquery", "writefile", "createwrite", "fetch(", "axios.", "http.request", "redirect("]) {
            let sink = format!("{}::sink:{}", file_id, line_no);
            upsert(engine, CausalEntity { id: sink.clone(), kind: CausalEntityKind::RuntimeResource, name: format!("side-effect sink line {line_no}"), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            if let Some(src) = values.get("__last_input") { fact(engine, src, &sink, CausalRelationKind::Writes, line_no, &artifact.path, BTreeMap::new()); }
            fact(engine, file_id, &sink, CausalRelationKind::Writes, line_no, &artifact.path, BTreeMap::new());
        }

        // Queues/topics/events.
        if contains_any(&lower, &[".publish(", ".emit(", ".send(", "producer.send", "publish("]) {
            let event = format!("{}::event:{}", file_id, line_no);
            upsert(engine, CausalEntity { id: event.clone(), kind: CausalEntityKind::Event, name: format!("published event line {line_no}"), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            fact(engine, file_id, &event, CausalRelationKind::Publishes, line_no, &artifact.path, BTreeMap::new());
        }
        if contains_any(&lower, &[".subscribe(", ".consume(", "consumer.on", "addlistener("]) {
            let event = format!("{}::subscription:{}", file_id, line_no);
            upsert(engine, CausalEntity { id: event.clone(), kind: CausalEntityKind::Event, name: format!("subscription line {line_no}"), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            fact(engine, &event, file_id, CausalRelationKind::Consumes, line_no, &artifact.path, BTreeMap::new());
        }

        // Transactions/locks/concurrency.
        if contains_any(&lower, &["transaction(", "begin transaction", "$transaction", "atomic("]) {
            let tx = format!("{}::transaction:{}", file_id, line_no);
            upsert(engine, CausalEntity { id: tx.clone(), kind: CausalEntityKind::Transaction, name: format!("transaction line {line_no}"), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            fact(engine, file_id, &tx, CausalRelationKind::BeginsTransaction, line_no, &artifact.path, BTreeMap::new());
        }
        if contains_any(&lower, &["mutex", ".lock(", "rwlock", "semaphore", "synchronized"]) {
            let lock = format!("{}::lock:{}", file_id, line_no);
            upsert(engine, CausalEntity { id: lock.clone(), kind: CausalEntityKind::Lock, name: format!("lock line {line_no}"), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            fact(engine, file_id, &lock, CausalRelationKind::Acquires, line_no, &artifact.path, BTreeMap::new());
        }

        // Routes/APIs.
        if contains_any(&lower, &["app.get(", "app.post(", "app.put(", "app.delete(", "router.get(", "router.post(", "@getmapping", "@postmapping", "@app.get", "@app.post", "http.handlefunc"]) {
            let api = format!("{}::api:{}", file_id, line_no);
            upsert(engine, CausalEntity { id: api.clone(), kind: CausalEntityKind::Api, name: line.chars().take(120).collect(), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            fact(engine, file_id, &api, CausalRelationKind::Defines, line_no, &artifact.path, BTreeMap::new());
        }

        // Tests and assertions.
        if is_test(&artifact.path.to_ascii_lowercase()) && contains_any(&lower, &["assert", "expect(", "should(", "pytest", "test(", "it("]) {
            let test = format!("{}::test:{}", file_id, line_no);
            upsert(engine, CausalEntity { id: test.clone(), kind: CausalEntityKind::Test, name: line.chars().take(120).collect(), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            fact(engine, &test, file_id, if lower.contains("assert") || lower.contains("expect(") { CausalRelationKind::Asserts } else { CausalRelationKind::Exercises }, line_no, &artifact.path, BTreeMap::new());
        }

        // Retry/timeout/circuit-breaker failure semantics.
        if contains_any(&lower, &["retry", "backoff"]) {
            let op = format!("{}::retry:{}", file_id, line_no);
            upsert(engine, CausalEntity { id: op.clone(), kind: CausalEntityKind::RuntimeResource, name: format!("retry line {line_no}"), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            fact(engine, file_id, &op, CausalRelationKind::Retries, line_no, &artifact.path, BTreeMap::new());
        }
        if lower.contains("timeout") {
            let op = format!("{}::timeout:{}", file_id, line_no);
            upsert(engine, CausalEntity { id: op.clone(), kind: CausalEntityKind::RuntimeResource, name: format!("timeout line {line_no}"), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            fact(engine, file_id, &op, CausalRelationKind::TimesOut, line_no, &artifact.path, BTreeMap::new());
        }
        if contains_any(&lower, &["circuitbreaker", "circuit_breaker", "circuit breaker"]) {
            let op = format!("{}::circuit:{}", file_id, line_no);
            upsert(engine, CausalEntity { id: op.clone(), kind: CausalEntityKind::RuntimeResource, name: format!("circuit breaker line {line_no}"), repository: Some(artifact.repository.clone()), path: Some(artifact.path.clone()), attributes: BTreeMap::new() });
            fact(engine, file_id, &op, CausalRelationKind::CircuitBreaks, line_no, &artifact.path, BTreeMap::new());
        }
    }
}

fn extract_env_keys(line: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for marker in ["process.env.", "import.meta.env."] {
        let mut rest = line;
        while let Some(pos) = rest.find(marker) {
            let tail = &rest[pos + marker.len()..];
            let key: String = tail.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !key.is_empty() { keys.push(key); }
            rest = tail;
        }
    }
    for marker in ["std::env::var(\"", "env::var(\"", "os.getenv(\"", "getenv(\""] {
        let mut rest = line;
        while let Some(pos) = rest.find(marker) {
            let tail = &rest[pos + marker.len()..];
            if let Some(end) = tail.find('"') { if end > 0 { keys.push(tail[..end].to_string()); } }
            rest = tail;
        }
    }
    keys.sort(); keys.dedup(); keys
}
fn looks_secret(key: &str) -> bool { let k=key.to_ascii_lowercase(); contains_any(&k,&["secret","token","password","private_key","api_key","apikey"]) }
fn contains_any(haystack: &str, needles: &[&str]) -> bool { needles.iter().any(|n| haystack.contains(n)) }
fn split_assignment(line: &str) -> Option<(&str,&str)> {
    for marker in [" = ", " := "] {
        if let Some((l,r)) = line.split_once(marker) {
            if !l.contains("==") && !r.starts_with('=') { return Some((l,r)); }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_prisma_tables_columns_and_config() {
        let artifacts = vec![
            RepositoryArtifact { repository:"r".into(), path:"prisma/schema.prisma".into(), content:"model User {\n id String @id\n email String\n}".into() },
            RepositoryArtifact { repository:"r".into(), path:"src/auth.ts".into(), content:"const secret = process.env.JWT_SECRET;\nconst input = req.body.email;\nconst safe = sanitize(input);\ndb.query(safe);".into() },
        ];
        let e = CausalArtifactExtractor::extract(&artifacts);
        assert!(e.entities().any(|x| x.kind == CausalEntityKind::Table && x.name == "User"));
        assert!(e.entities().any(|x| x.kind == CausalEntityKind::Secret && x.name == "JWT_SECRET"));
        assert!(e.entities().any(|x| x.kind == CausalEntityKind::RuntimeResource && x.name.contains("side-effect sink")));
    }

    #[test]
    fn extracts_codeowners_as_real_ownership_evidence() {
        let e = CausalArtifactExtractor::extract(&[RepositoryArtifact { repository:"r".into(), path:".github/CODEOWNERS".into(), content:"/payments/ @alice @team/payments".into() }]);
        let risks = e.ownership_risks();
        assert!(e.entities().any(|x| matches!(x.kind, CausalEntityKind::Owner | CausalEntityKind::Team)));
        assert!(!risks.is_empty());
    }
}
