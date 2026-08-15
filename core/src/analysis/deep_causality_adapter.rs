//! Adapters from the existing CKB dependency/runtime graph into V13.1 causal facts.
//! Classification is deliberately conservative: path/name/metadata evidence can
//! refine an entity kind, but absence of such evidence remains a normal code
//! entity rather than an invented database, queue, service or deployment.

use super::deep_causality::*;
use crate::graph::DependencyGraph;
use crate::types::{EdgeKind, Node, NodeKind};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone)]
pub struct CausalGraphAdapter<'a> {
    graph: &'a DependencyGraph,
    repository: Option<String>,
}

impl<'a> CausalGraphAdapter<'a> {
    pub fn new(graph: &'a DependencyGraph) -> Self {
        Self { graph, repository: None }
    }

    pub fn repository(mut self, repository: impl Into<String>) -> Self {
        self.repository = Some(repository.into());
        self
    }

    pub fn build(&self) -> DeepCausalityEngine {
        let mut engine = DeepCausalityEngine::new();
        let mut ids: HashMap<String, String> = HashMap::new();
        for node in self.graph.nodes() {
            let id = self.causal_id(node);
            ids.insert(node.id.0.clone(), id.clone());
            engine.upsert_entity(self.entity_for_node(node, id));
        }
        for edge in self.graph.edges() {
            let Some(from) = ids.get(&edge.from.0).cloned() else { continue; };
            let Some(to) = ids.get(&edge.to.0).cloned() else { continue; };
            let relation = match edge.kind {
                EdgeKind::Import => CausalRelationKind::Imports,
                EdgeKind::Calls => CausalRelationKind::Calls,
                EdgeKind::Returns => CausalRelationKind::Returns,
                EdgeKind::Parameter => CausalRelationKind::Assigns,
                EdgeKind::Property => CausalRelationKind::DependsOn,
                EdgeKind::Extends | EdgeKind::Implements | EdgeKind::Instantiates => CausalRelationKind::DependsOn,
            };
            let mut metadata = edge.metadata.iter().map(|(key, value)| (key.clone(), value.clone())).collect::<BTreeMap<_, _>>();
            metadata.insert("ckb.edge_kind".into(), format!("{:?}", edge.kind));
            metadata.insert("ckb.raw_from".into(), edge.from.0.clone());
            metadata.insert("ckb.raw_to".into(), edge.to.0.clone());
            let _ = engine.add_fact(CausalFact {
                from,
                to,
                relation,
                evidence: CausalEvidenceClass::Static,
                confidence: edge.weight.clamp(0.0, 1.0),
                condition: edge.metadata.get("condition").cloned(),
                timestamp_ms: None,
                metadata,
            });
        }
        engine
    }

    fn causal_id(&self, node: &Node) -> String {
        let Some(repository) = self.repository.as_deref() else { return node.id.0.clone(); };
        let path = node.path.to_string_lossy().replace('\\', "/").trim_start_matches("./").to_string();
        if node.kind == NodeKind::File {
            format!("repo:{repository}::file:{path}")
        } else {
            format!("repo:{repository}::node:{}", node.id.0)
        }
    }

    fn entity_for_node(&self, node: &Node, id: String) -> CausalEntity {
        let path = node.path.to_string_lossy().replace('\\', "/");
        let lower_path = path.to_ascii_lowercase();
        let lower_name = node.name.to_ascii_lowercase();
        let metadata = node.metadata.iter().map(|(key, value)| (key.clone(), value.clone())).collect::<BTreeMap<_, _>>();
        let kind = classify_node(node, &lower_path, &lower_name);
        let mut attributes = metadata;
        attributes.insert("ckb.raw_node_id".into(), node.id.0.clone());

        if let Some(runtime) = self.graph.get_runtime_metrics(&node.id) {
            attributes.insert("runtime.execution_count".into(), runtime.execution_count.to_string());
            attributes.insert("runtime.latency_ms".into(), runtime.avg_latency_ms.to_string());
            attributes.insert("runtime.error_rate".into(), runtime.error_rate.to_string());
            attributes.insert("runtime.hotpath".into(), runtime.is_hotpath.to_string());
        }

        CausalEntity {
            id,
            kind,
            name: node.name.clone(),
            repository: self.repository.clone(),
            path: Some(path),
            attributes,
        }
    }
}

fn classify_node(node: &Node, path: &str, name: &str) -> CausalEntityKind {
    let meta = |key: &str| node.metadata.get(key).map(|value| value.to_ascii_lowercase());
    let declared = meta("ckb.entity_kind").or_else(|| meta("entity_kind")).or_else(|| meta("kind"));
    if let Some(kind) = declared.as_deref() {
        if let Some(mapped) = declared_kind(kind) { return mapped; }
    }

    if path.contains("/migrations/") || path.contains("/migration/") || name.contains("migration") { return CausalEntityKind::Migration; }
    if path.ends_with("schema.prisma") || path.ends_with(".sql") || path.contains("/schema/") { return CausalEntityKind::Schema; }
    if path.ends_with("docker-compose.yml") || path.ends_with("docker-compose.yaml") || path.ends_with("dockerfile") || path.ends_with("compose.yml") || path.ends_with("compose.yaml") || path.ends_with(".tf") || path.ends_with(".tf.json") || path.contains("/k8s/") || path.contains("/kubernetes/") || path.contains("/helm/") { return CausalEntityKind::Infrastructure; }
    if path.contains("/.github/workflows/") || path.contains("/deploy/") || path.contains("/deployment/") { return CausalEntityKind::Deployment; }
    if path.contains("/test/") || path.contains("/tests/") || path.starts_with("tests/") || path.contains("/__tests__/") || path.ends_with("_test.go") || path.ends_with("_test.rs") || path.ends_with("_test.py") || path.ends_with(".test.ts") || path.ends_with(".test.tsx") || path.ends_with(".spec.ts") || path.ends_with(".spec.tsx") { return CausalEntityKind::Test; }
    if name.starts_with("feature_") || name.contains("featureflag") || name.contains("feature_flag") || node.metadata.contains_key("feature_flag") { return CausalEntityKind::FeatureFlag; }
    if name.contains("queue") { return CausalEntityKind::Queue; }
    if name.contains("topic") { return CausalEntityKind::Topic; }
    if name.ends_with("event") || name.contains("event_") { return CausalEntityKind::Event; }
    if name.contains("cron") || name.contains("scheduler") || name.ends_with("job") { return CausalEntityKind::Job; }
    if name.contains("transaction") { return CausalEntityKind::Transaction; }
    if name.contains("mutex") || name.contains("lock") { return CausalEntityKind::Lock; }
    if name.ends_with("controller") || name.ends_with("route") || name.ends_with("endpoint") { return CausalEntityKind::Api; }
    if name.ends_with("service") { return CausalEntityKind::Service; }
    if name.contains("config") || name.starts_with("env_") || name.starts_with("ckb_") && node.kind == NodeKind::Variable { return CausalEntityKind::Configuration; }
    if name.contains("secret") || name.contains("token") || name.contains("password") || name.contains("api_key") { return CausalEntityKind::Secret; }

    match node.kind {
        NodeKind::File => CausalEntityKind::File,
        NodeKind::Module | NodeKind::Namespace => CausalEntityKind::Package,
        NodeKind::Variable => CausalEntityKind::Value,
        NodeKind::Class | NodeKind::Interface | NodeKind::Enum | NodeKind::Function | NodeKind::Method | NodeKind::Type => CausalEntityKind::Symbol,
    }
}

fn declared_kind(value: &str) -> Option<CausalEntityKind> {
    Some(match value.trim().replace('-', "_").as_str() {
        "repository" | "repo" => CausalEntityKind::Repository,
        "service" => CausalEntityKind::Service,
        "package" | "module" => CausalEntityKind::Package,
        "file" => CausalEntityKind::File,
        "symbol" | "function" | "method" | "class" => CausalEntityKind::Symbol,
        "parameter" | "param" => CausalEntityKind::Parameter,
        "value" | "variable" => CausalEntityKind::Value,
        "api" | "endpoint" | "route" => CausalEntityKind::Api,
        "schema" => CausalEntityKind::Schema,
        "table" => CausalEntityKind::Table,
        "column" => CausalEntityKind::Column,
        "migration" => CausalEntityKind::Migration,
        "data_store" | "datastore" | "database" => CausalEntityKind::DataStore,
        "queue" => CausalEntityKind::Queue,
        "topic" => CausalEntityKind::Topic,
        "event" => CausalEntityKind::Event,
        "job" | "cron" => CausalEntityKind::Job,
        "feature_flag" => CausalEntityKind::FeatureFlag,
        "configuration" | "config" | "env" => CausalEntityKind::Configuration,
        "secret" => CausalEntityKind::Secret,
        "infrastructure" | "infra" => CausalEntityKind::Infrastructure,
        "deployment" => CausalEntityKind::Deployment,
        "runtime_resource" => CausalEntityKind::RuntimeResource,
        "test" => CausalEntityKind::Test,
        "policy" => CausalEntityKind::Policy,
        "owner" => CausalEntityKind::Owner,
        "team" => CausalEntityKind::Team,
        "commit" => CausalEntityKind::Commit,
        "trace" => CausalEntityKind::Trace,
        "lock" | "mutex" => CausalEntityKind::Lock,
        "transaction" => CausalEntityKind::Transaction,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{NodeId, RuntimeMetrics};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn node(id: &str, path: &str, kind: NodeKind) -> Node {
        Node { id: NodeId(id.into()), kind, name: id.rsplit("::").next().unwrap_or(id).into(), path: PathBuf::from(path), line: 1, column: 0, exports: vec![], imports: vec![], metadata: HashMap::new() }
    }

    #[test]
    fn classifies_repository_artifacts_conservatively() {
        let schema = node("db::schema", "prisma/schema.prisma", NodeKind::File);
        let infra = node("infra::compose", "docker-compose.yml", NodeKind::File);
        let test = node("tests::auth", "tests/auth_test.rs", NodeKind::Function);
        assert_eq!(classify_node(&schema, "prisma/schema.prisma", "schema"), CausalEntityKind::Schema);
        assert_eq!(classify_node(&infra, "docker-compose.yml", "compose"), CausalEntityKind::Infrastructure);
        assert_eq!(classify_node(&test, "tests/auth_test.rs", "auth"), CausalEntityKind::Test);
    }

    #[test]
    fn repository_file_ids_match_artifact_namespace() {
        let graph = DependencyGraph::new();
        let adapter = CausalGraphAdapter::new(&graph).repository("acme/api");
        let file = node("src/a.ts::file", "src/a.ts", NodeKind::File);
        assert_eq!(adapter.causal_id(&file), "repo:acme/api::file:src/a.ts");
    }

    #[test]
    fn runtime_metrics_are_only_added_when_observed() {
        let mut graph = DependencyGraph::new();
        graph.record_runtime_metrics(NodeId("missing".into()), RuntimeMetrics { execution_count: 7, avg_latency_ms: 12.0, error_rate: 0.1, is_hotpath: false });
        let engine = CausalGraphAdapter::new(&graph).build();
        assert_eq!(engine.entities().count(), 0, "runtime evidence must not invent a source entity when graph identity is unresolved");
    }
}
