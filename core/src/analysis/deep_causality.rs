//! V13.1 Deep Software Causality
//!
//! A provider-neutral, evidence-preserving software reasoning layer.  This
//! module intentionally operates on explicit facts rather than inventing AST,
//! runtime, infrastructure or organizational evidence that was never observed.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CausalEvidenceClass {
    Static,
    Runtime,
    History,
    Validation,
    Human,
    Predicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CausalEntityKind {
    Repository,
    Service,
    Package,
    File,
    Symbol,
    Parameter,
    Value,
    Api,
    Schema,
    Table,
    Column,
    Migration,
    DataStore,
    Queue,
    Topic,
    Event,
    Job,
    FeatureFlag,
    Configuration,
    Secret,
    Infrastructure,
    Deployment,
    RuntimeResource,
    Test,
    Policy,
    Owner,
    Team,
    Commit,
    Trace,
    Lock,
    Transaction,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CausalRelationKind {
    Calls,
    Reads,
    Writes,
    Returns,
    Assigns,
    Derives,
    Sanitizes,
    Validates,
    TrustBoundary,
    DependsOn,
    Imports,
    Owns,
    Emits,
    Consumes,
    Publishes,
    Subscribes,
    Schedules,
    Guards,
    Enables,
    Disables,
    Acquires,
    Releases,
    BeginsTransaction,
    CommitsTransaction,
    RollsBackTransaction,
    Migrates,
    Defines,
    Deploys,
    RoutesTo,
    ConnectsTo,
    Observes,
    Exercises,
    Asserts,
    Violates,
    Changes,
    Supersedes,
    Reviews,
    AuthoredBy,
    Allocates,
    WaitsFor,
    Retries,
    TimesOut,
    CircuitBreaks,
    CorrelatesWith,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalEntity {
    pub id: String,
    pub kind: CausalEntityKind,
    pub name: String,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalFact {
    pub from: String,
    pub to: String,
    pub relation: CausalRelationKind,
    pub evidence: CausalEvidenceClass,
    #[serde(default = "one")]
    pub confidence: f32,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub timestamp_ms: Option<i64>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

fn one() -> f32 { 1.0 }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalPath {
    pub entities: Vec<String>,
    pub facts: Vec<CausalFact>,
    pub minimum_confidence: f32,
    pub evidence_classes: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaintFinding {
    pub source: String,
    pub sink: String,
    pub path: CausalPath,
    pub crossed_trust_boundary: bool,
    pub sanitizer_observed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConcurrencyHazard {
    pub resource: String,
    pub writers: Vec<String>,
    pub locks: Vec<String>,
    pub transactions: Vec<String>,
    pub kind: String,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractField {
    pub name: String,
    pub required: bool,
    pub type_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiContract {
    pub id: String,
    pub fields: Vec<ContractField>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractChange {
    pub field: String,
    pub classification: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchitectureRule {
    pub id: String,
    pub description: String,
    pub from_kind: Option<CausalEntityKind>,
    pub to_kind: Option<CausalEntityKind>,
    pub forbidden_relation: CausalRelationKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyViolation {
    pub rule_id: String,
    pub fact: CausalFact,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeOperation {
    pub operation: String,
    pub entity_id: String,
    #[serde(default)]
    pub replacement_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeSimulation {
    pub affected_entities: Vec<String>,
    pub affected_tests: Vec<String>,
    pub affected_contracts: Vec<String>,
    pub affected_deployments: Vec<String>,
    pub predicted_failures: Vec<String>,
    pub evidence: CausalEvidenceClass,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeHotspot {
    pub entity_id: String,
    pub cpu_ms: f64,
    pub memory_bytes: u64,
    pub latency_ms: f64,
    pub error_rate: f64,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnershipRisk {
    pub entity_id: String,
    pub active_owners: Vec<String>,
    pub bus_factor: usize,
    pub risk: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchitectureQualityMetrics {
    pub entities: usize,
    pub relations: usize,
    pub cycles: usize,
    pub average_fan_in: f64,
    pub average_fan_out: f64,
    pub max_fan_in: usize,
    pub max_fan_out: usize,
    pub instability_by_entity: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeepCausalityEngine {
    entities: HashMap<String, CausalEntity>,
    facts: Vec<CausalFact>,
}

impl DeepCausalityEngine {
    pub fn new() -> Self { Self::default() }

    pub fn from_facts(entities: Vec<CausalEntity>, facts: Vec<CausalFact>) -> Self {
        Self {
            entities: entities.into_iter().map(|e| (e.id.clone(), e)).collect(),
            facts,
        }
    }

    pub fn upsert_entity(&mut self, entity: CausalEntity) {
        self.entities.insert(entity.id.clone(), entity);
    }

    pub fn add_fact(&mut self, fact: CausalFact) -> Result<(), String> {
        if !self.entities.contains_key(&fact.from) || !self.entities.contains_key(&fact.to) {
            return Err(format!("causal fact references unknown entity: {} -> {}", fact.from, fact.to));
        }
        if !(0.0..=1.0).contains(&fact.confidence) {
            return Err("confidence must be within 0..=1".into());
        }
        self.facts.push(fact);
        Ok(())
    }

    pub fn entities(&self) -> impl Iterator<Item = &CausalEntity> { self.entities.values() }
    pub fn facts(&self) -> &[CausalFact] { &self.facts }

    fn outgoing<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a CausalFact> + 'a {
        self.facts.iter().filter(move |f| f.from == id)
    }

    fn incoming<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a CausalFact> + 'a {
        self.facts.iter().filter(move |f| f.to == id)
    }

    fn path_with_filter<F>(&self, source: &str, sink: &str, max_depth: usize, allow: F) -> Option<CausalPath>
    where F: Fn(&CausalFact) -> bool {
        if source == sink {
            return Some(CausalPath { entities: vec![source.to_string()], facts: vec![], minimum_confidence: 1.0, evidence_classes: BTreeSet::new() });
        }
        let mut queue = VecDeque::from([(source.to_string(), vec![source.to_string()], Vec::<CausalFact>::new())]);
        let mut best_depth: HashMap<String, usize> = HashMap::from([(source.to_string(), 0)]);
        while let Some((id, entities, path_facts)) = queue.pop_front() {
            if path_facts.len() >= max_depth { continue; }
            for fact in self.outgoing(&id).filter(|f| allow(f)) {
                let depth = path_facts.len() + 1;
                if best_depth.get(&fact.to).map(|d| *d < depth).unwrap_or(false) { continue; }
                let mut next_entities = entities.clone();
                next_entities.push(fact.to.clone());
                let mut next_facts = path_facts.clone();
                next_facts.push(fact.clone());
                if fact.to == sink {
                    let min_conf = next_facts.iter().map(|f| f.confidence).fold(1.0_f32, f32::min);
                    let evidence_classes = next_facts.iter().map(|f| format!("{:?}", f.evidence).to_ascii_lowercase()).collect();
                    return Some(CausalPath { entities: next_entities, facts: next_facts, minimum_confidence: min_conf, evidence_classes });
                }
                best_depth.insert(fact.to.clone(), depth);
                queue.push_back((fact.to.clone(), next_entities, next_facts));
            }
        }
        None
    }

    /// 1. Interprocedural data-flow intelligence.
    pub fn data_flow_path(&self, source: &str, sink: &str, max_depth: usize) -> Option<CausalPath> {
        self.path_with_filter(source, sink, max_depth, |f| matches!(f.relation,
            CausalRelationKind::Assigns | CausalRelationKind::Derives | CausalRelationKind::Reads |
            CausalRelationKind::Writes | CausalRelationKind::Returns | CausalRelationKind::Calls |
            CausalRelationKind::Emits | CausalRelationKind::Consumes | CausalRelationKind::RoutesTo))
    }

    /// 2. Taint/trust-boundary analysis. Sanitization is evidence, not assumption.
    pub fn taint_paths(&self, sources: &[String], sinks: &[String], max_depth: usize) -> Vec<TaintFinding> {
        let mut out = Vec::new();
        for source in sources {
            for sink in sinks {
                if let Some(path) = self.data_flow_path(source, sink, max_depth) {
                    let crossed = path.facts.iter().any(|f| f.relation == CausalRelationKind::TrustBoundary);
                    let sanitized = path.facts.iter().any(|f| f.relation == CausalRelationKind::Sanitizes);
                    if !sanitized {
                        out.push(TaintFinding { source: source.clone(), sink: sink.clone(), path, crossed_trust_boundary: crossed, sanitizer_observed: false });
                    }
                }
            }
        }
        out
    }

    /// 3. Control-flow/path-sensitive reachability over recorded branch conditions.
    pub fn reachable_under(&self, source: &str, sink: &str, required_conditions: &[String], max_depth: usize) -> Option<CausalPath> {
        let required: HashSet<&str> = required_conditions.iter().map(String::as_str).collect();
        self.path_with_filter(source, sink, max_depth, |f| {
            f.condition.as_deref().map(|c| required.is_empty() || required.contains(c)).unwrap_or(true)
        })
    }

    /// 4. Bounded symbolic/constraint reasoning. Contradictory exact predicates are rejected.
    pub fn constraints_satisfiable(&self, constraints: &[String]) -> bool {
        let mut equals: HashMap<String, String> = HashMap::new();
        let mut not_equals: HashMap<String, HashSet<String>> = HashMap::new();
        for raw in constraints {
            let c = raw.trim();
            if let Some((left, right)) = c.split_once("!=") {
                let (left, right) = (left.trim().to_string(), right.trim().to_string());
                if equals.get(&left) == Some(&right) { return false; }
                not_equals.entry(left).or_default().insert(right);
            } else if let Some((left, right)) = c.split_once("==") {
                let (left, right) = (left.trim().to_string(), right.trim().to_string());
                if equals.get(&left).map(|v| v != &right).unwrap_or(false) { return false; }
                if not_equals.get(&left).map(|s| s.contains(&right)).unwrap_or(false) { return false; }
                equals.insert(left, right);
            }
        }
        true
    }

    /// 5. Concurrency intelligence: unprotected multi-writer state and lock-order cycles.
    pub fn concurrency_hazards(&self) -> Vec<ConcurrencyHazard> {
        let mut out = Vec::new();
        for resource in self.entities.values().filter(|e| matches!(e.kind, CausalEntityKind::DataStore | CausalEntityKind::RuntimeResource | CausalEntityKind::Value)) {
            let writers: Vec<_> = self.incoming(&resource.id).filter(|f| f.relation == CausalRelationKind::Writes).map(|f| f.from.clone()).collect();
            if writers.len() > 1 {
                let writer_set: HashSet<_> = writers.iter().cloned().collect();
                let locks: BTreeSet<_> = self.facts.iter().filter(|f| writer_set.contains(&f.from) && f.relation == CausalRelationKind::Acquires).map(|f| f.to.clone()).collect();
                let tx: BTreeSet<_> = self.facts.iter().filter(|f| writer_set.contains(&f.from) && f.relation == CausalRelationKind::BeginsTransaction).map(|f| f.to.clone()).collect();
                if locks.is_empty() && tx.is_empty() {
                    out.push(ConcurrencyHazard { resource: resource.id.clone(), writers, locks: vec![], transactions: vec![], kind: "unprotected_multi_writer".into(), rationale: "Multiple observed/static writers exist without recorded lock or transaction protection.".into() });
                }
            }
        }
        for cycle in self.cycles_for_relations(&[CausalRelationKind::WaitsFor, CausalRelationKind::Acquires]) {
            out.push(ConcurrencyHazard { resource: cycle.join(" -> "), writers: vec![], locks: cycle, transactions: vec![], kind: "potential_deadlock".into(), rationale: "A lock/wait dependency cycle is present in recorded concurrency facts.".into() });
        }
        out
    }

    /// 6. Database/schema/migration first-class impact intelligence.
    pub fn schema_impact(&self, schema_entity: &str, max_depth: usize) -> Vec<String> {
        self.reverse_impact(schema_entity, max_depth, |e| matches!(e.kind, CausalEntityKind::Symbol | CausalEntityKind::Api | CausalEntityKind::Test | CausalEntityKind::Migration | CausalEntityKind::Service))
    }

    /// 7. Infrastructure-as-code/deployment impact.
    pub fn infrastructure_impact(&self, infra_entity: &str, max_depth: usize) -> Vec<String> {
        self.reverse_impact(infra_entity, max_depth, |e| matches!(e.kind, CausalEntityKind::Service | CausalEntityKind::Deployment | CausalEntityKind::Repository | CausalEntityKind::RuntimeResource))
    }

    /// 8. Configuration and feature-flag causality.
    pub fn config_dependents(&self, config_entity: &str, max_depth: usize) -> Vec<String> {
        self.reverse_impact(config_entity, max_depth, |_| true)
    }

    /// 9. Event-driven/distributed-system semantics.
    pub fn distributed_flow(&self, source: &str, sink: &str, max_depth: usize) -> Option<CausalPath> {
        self.path_with_filter(source, sink, max_depth, |f| matches!(f.relation,
            CausalRelationKind::Emits | CausalRelationKind::Consumes | CausalRelationKind::Publishes |
            CausalRelationKind::Subscribes | CausalRelationKind::Schedules | CausalRelationKind::RoutesTo |
            CausalRelationKind::Retries | CausalRelationKind::Calls | CausalRelationKind::Writes))
    }

    /// 10. API/schema evolution compatibility classification.
    pub fn compare_contracts(&self, before: &ApiContract, after: &ApiContract) -> Vec<ContractChange> {
        let old: HashMap<_, _> = before.fields.iter().map(|f| (f.name.as_str(), f)).collect();
        let new: HashMap<_, _> = after.fields.iter().map(|f| (f.name.as_str(), f)).collect();
        let mut changes = Vec::new();
        for (name, field) in &old {
            match new.get(name) {
                None => changes.push(ContractChange { field: (*name).to_string(), classification: "breaking".into(), reason: "field removed".into() }),
                Some(next) if next.type_name != field.type_name => changes.push(ContractChange { field: (*name).to_string(), classification: "breaking".into(), reason: format!("type changed {} -> {}", field.type_name, next.type_name) }),
                Some(next) if !field.required && next.required => changes.push(ContractChange { field: (*name).to_string(), classification: "breaking".into(), reason: "optional field became required".into() }),
                _ => {}
            }
        }
        for (name, field) in &new {
            if !old.contains_key(name) {
                changes.push(ContractChange { field: (*name).to_string(), classification: if field.required { "potentially_breaking" } else { "compatible" }.into(), reason: if field.required { "new required field" } else { "new optional field" }.into() });
            }
        }
        changes.sort_by(|a, b| a.field.cmp(&b.field));
        changes
    }

    /// 11. Behavioral test intelligence: choose tests connected to changed entities.
    pub fn tests_for_change(&self, changed: &[String], max_depth: usize) -> Vec<String> {
        let mut tests = BTreeSet::new();
        for id in changed {
            for affected in self.reverse_impact(id, max_depth, |e| e.kind == CausalEntityKind::Test) { tests.insert(affected); }
            for fact in self.facts.iter().filter(|f| f.from == *id || f.to == *id) {
                let other = if fact.from == *id { &fact.to } else { &fact.from };
                if self.entities.get(other).map(|e| e.kind == CausalEntityKind::Test).unwrap_or(false) { tests.insert(other.clone()); }
            }
        }
        tests.into_iter().collect()
    }

    /// 12. Executable architecture invariant enforcement.
    pub fn enforce_rules(&self, rules: &[ArchitectureRule]) -> Vec<PolicyViolation> {
        let mut out = Vec::new();
        for fact in &self.facts {
            let Some(from) = self.entities.get(&fact.from) else { continue; };
            let Some(to) = self.entities.get(&fact.to) else { continue; };
            for rule in rules {
                if fact.relation != rule.forbidden_relation { continue; }
                if rule.from_kind.as_ref().map(|k| k != &from.kind).unwrap_or(false) { continue; }
                if rule.to_kind.as_ref().map(|k| k != &to.kind).unwrap_or(false) { continue; }
                out.push(PolicyViolation { rule_id: rule.id.clone(), fact: fact.clone(), message: rule.description.clone() });
            }
        }
        out
    }

    /// 13. Architecture drift forecasting from historical relation counts.
    pub fn forecast_drift(&self, historical_edge_counts: &[usize], horizon: usize) -> Vec<f64> {
        if historical_edge_counts.is_empty() || horizon == 0 { return vec![]; }
        if historical_edge_counts.len() == 1 { return vec![historical_edge_counts[0] as f64; horizon]; }
        let deltas: Vec<f64> = historical_edge_counts.windows(2).map(|w| w[1] as f64 - w[0] as f64).collect();
        let trend = deltas.iter().sum::<f64>() / deltas.len() as f64;
        let base = *historical_edge_counts.last().unwrap() as f64;
        (1..=horizon).map(|n| (base + trend * n as f64).max(0.0)).collect()
    }

    /// 14. Proposed-change simulation. Output is explicitly PREDICTED.
    pub fn simulate_change(&self, operations: &[ChangeOperation], max_depth: usize) -> ChangeSimulation {
        let changed: Vec<String> = operations.iter().map(|o| o.entity_id.clone()).collect();
        let mut affected = BTreeSet::new();
        for id in &changed {
            affected.insert(id.clone());
            for e in self.reverse_impact(id, max_depth, |_| true) { affected.insert(e); }
        }
        let affected_vec: Vec<_> = affected.iter().cloned().collect();
        let tests = affected_vec.iter().filter(|id| self.entities.get(*id).map(|e| e.kind == CausalEntityKind::Test).unwrap_or(false)).cloned().collect();
        let contracts = affected_vec.iter().filter(|id| self.entities.get(*id).map(|e| matches!(e.kind, CausalEntityKind::Api | CausalEntityKind::Schema)).unwrap_or(false)).cloned().collect();
        let deployments = affected_vec.iter().filter(|id| self.entities.get(*id).map(|e| e.kind == CausalEntityKind::Deployment).unwrap_or(false)).cloned().collect();
        let failures = operations.iter().filter(|o| o.operation == "delete").flat_map(|o| self.incoming(&o.entity_id).map(move |f| format!("{} may lose {:?} target {}", f.from, f.relation, o.entity_id))).collect();
        ChangeSimulation { affected_entities: affected_vec, affected_tests: tests, affected_contracts: contracts, affected_deployments: deployments, predicted_failures: failures, evidence: CausalEvidenceClass::Predicted }
    }

    /// 15. Runtime/resource intelligence. Requires observed Runtime entities/attributes.
    pub fn runtime_hotspots(&self) -> Vec<RuntimeHotspot> {
        let mut out = Vec::new();
        for entity in self.entities.values().filter(|e| matches!(e.kind, CausalEntityKind::Symbol | CausalEntityKind::Service | CausalEntityKind::RuntimeResource)) {
            let parse = |k: &str| entity.attributes.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
            let cpu = parse("runtime.cpu_ms");
            let mem = entity.attributes.get("runtime.memory_bytes").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            let latency = parse("runtime.latency_ms");
            let errors = parse("runtime.error_rate");
            if cpu == 0.0 && mem == 0 && latency == 0.0 && errors == 0.0 { continue; }
            let score = cpu.log10().max(0.0) + (mem as f64 + 1.0).log10() * 0.2 + latency.log10().max(0.0) + errors * 10.0;
            out.push(RuntimeHotspot { entity_id: entity.id.clone(), cpu_ms: cpu, memory_bytes: mem, latency_ms: latency, error_rate: errors, score });
        }
        out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
        out
    }

    /// 16. Failure propagation modeling including retries/timeouts/routes/dependencies.
    pub fn failure_propagation(&self, source: &str, max_depth: usize) -> Vec<String> {
        self.forward_impact(source, max_depth, |f| matches!(f.relation,
            CausalRelationKind::DependsOn | CausalRelationKind::Calls | CausalRelationKind::RoutesTo |
            CausalRelationKind::Consumes | CausalRelationKind::Retries | CausalRelationKind::TimesOut |
            CausalRelationKind::ConnectsTo))
    }

    /// 17. Temporal architecture: compare fact identity across snapshots.
    pub fn temporal_diff(&self, older: &DeepCausalityEngine) -> (Vec<CausalFact>, Vec<CausalFact>) {
        fn key(f: &CausalFact) -> String { format!("{}|{:?}|{}|{:?}|{:?}", f.from, f.relation, f.to, f.evidence, f.condition) }
        let old: HashSet<_> = older.facts.iter().map(key).collect();
        let now: HashSet<_> = self.facts.iter().map(key).collect();
        let added = self.facts.iter().filter(|f| !old.contains(&key(f))).cloned().collect();
        let removed = older.facts.iter().filter(|f| !now.contains(&key(f))).cloned().collect();
        (added, removed)
    }

    /// 18. Cross-repository/organization architecture paths.
    pub fn cross_repo_path(&self, source: &str, sink: &str, max_depth: usize) -> Option<CausalPath> {
        let path = self.path_with_filter(source, sink, max_depth, |_| true)?;
        let repos: HashSet<_> = path.entities.iter().filter_map(|id| self.entities.get(id)?.repository.clone()).collect();
        if repos.len() > 1 { Some(path) } else { None }
    }

    /// 19. Ownership/socio-technical intelligence.
    pub fn ownership_risks(&self) -> Vec<OwnershipRisk> {
        let mut out = Vec::new();
        for entity in self.entities.values().filter(|e| matches!(e.kind, CausalEntityKind::Service | CausalEntityKind::Package | CausalEntityKind::Repository | CausalEntityKind::File)) {
            let mut owners = BTreeSet::new();
            for fact in self.incoming(&entity.id).chain(self.outgoing(&entity.id)) {
                let candidate = if fact.from == entity.id { &fact.to } else { &fact.from };
                if matches!(fact.relation, CausalRelationKind::Owns | CausalRelationKind::AuthoredBy | CausalRelationKind::Reviews) && self.entities.get(candidate).map(|e| matches!(e.kind, CausalEntityKind::Owner | CausalEntityKind::Team)).unwrap_or(false) {
                    owners.insert(candidate.clone());
                }
            }
            let bus_factor = owners.len();
            let risk = match bus_factor { 0 => "unowned", 1 => "critical_single_owner", 2 => "elevated", _ => "distributed" }.to_string();
            out.push(OwnershipRisk { entity_id: entity.id.clone(), active_owners: owners.into_iter().collect(), bus_factor, risk });
        }
        out
    }

    /// 20. Evidence-based architecture quality metrics.
    pub fn quality_metrics(&self) -> ArchitectureQualityMetrics {
        let mut fan_in: HashMap<String, usize> = self.entities.keys().map(|k| (k.clone(), 0)).collect();
        let mut fan_out = fan_in.clone();
        for fact in &self.facts {
            *fan_out.entry(fact.from.clone()).or_default() += 1;
            *fan_in.entry(fact.to.clone()).or_default() += 1;
        }
        let n = self.entities.len().max(1) as f64;
        let average_fan_in = fan_in.values().sum::<usize>() as f64 / n;
        let average_fan_out = fan_out.values().sum::<usize>() as f64 / n;
        let max_fan_in = fan_in.values().copied().max().unwrap_or(0);
        let max_fan_out = fan_out.values().copied().max().unwrap_or(0);
        let instability_by_entity = self.entities.keys().map(|id| {
            let inc = *fan_in.get(id).unwrap_or(&0) as f64;
            let out = *fan_out.get(id).unwrap_or(&0) as f64;
            let denom = inc + out;
            (id.clone(), if denom == 0.0 { 0.0 } else { out / denom })
        }).collect();
        ArchitectureQualityMetrics {
            entities: self.entities.len(), relations: self.facts.len(), cycles: self.cycles_for_relations(&[]).len(),
            average_fan_in, average_fan_out, max_fan_in, max_fan_out, instability_by_entity,
        }
    }

    fn reverse_impact<F>(&self, source: &str, max_depth: usize, include: F) -> Vec<String>
    where F: Fn(&CausalEntity) -> bool {
        let mut seen = HashSet::from([source.to_string()]);
        let mut queue = VecDeque::from([(source.to_string(), 0usize)]);
        let mut out = BTreeSet::new();
        while let Some((id, depth)) = queue.pop_front() {
            if depth >= max_depth { continue; }
            for fact in self.incoming(&id) {
                if seen.insert(fact.from.clone()) {
                    if self.entities.get(&fact.from).map(&include).unwrap_or(false) { out.insert(fact.from.clone()); }
                    queue.push_back((fact.from.clone(), depth + 1));
                }
            }
        }
        out.into_iter().collect()
    }

    fn forward_impact<F>(&self, source: &str, max_depth: usize, allow: F) -> Vec<String>
    where F: Fn(&CausalFact) -> bool {
        let mut seen = HashSet::from([source.to_string()]);
        let mut queue = VecDeque::from([(source.to_string(), 0usize)]);
        let mut out = BTreeSet::new();
        while let Some((id, depth)) = queue.pop_front() {
            if depth >= max_depth { continue; }
            for fact in self.outgoing(&id).filter(|f| allow(f)) {
                if seen.insert(fact.to.clone()) {
                    out.insert(fact.to.clone());
                    queue.push_back((fact.to.clone(), depth + 1));
                }
            }
        }
        out.into_iter().collect()
    }

    fn cycles_for_relations(&self, relations: &[CausalRelationKind]) -> Vec<Vec<String>> {
        let allow_all = relations.is_empty();
        let allowed: HashSet<_> = relations.iter().cloned().collect();
        let mut cycles = BTreeSet::<Vec<String>>::new();
        for start in self.entities.keys() {
            let mut stack = vec![(start.clone(), vec![start.clone()])];
            while let Some((id, path)) = stack.pop() {
                if path.len() > 12 { continue; }
                for fact in self.outgoing(&id).filter(|f| allow_all || allowed.contains(&f.relation)) {
                    if fact.to == *start && path.len() > 1 {
                        let mut cycle = path.clone(); cycle.push(start.clone());
                        cycles.insert(cycle); continue;
                    }
                    if !path.contains(&fact.to) {
                        let mut next = path.clone(); next.push(fact.to.clone()); stack.push((fact.to.clone(), next));
                    }
                }
            }
        }
        cycles.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(id: &str, kind: CausalEntityKind) -> CausalEntity {
        CausalEntity { id: id.into(), kind, name: id.into(), repository: None, path: None, attributes: BTreeMap::new() }
    }
    fn fact(from: &str, to: &str, relation: CausalRelationKind) -> CausalFact {
        CausalFact { from: from.into(), to: to.into(), relation, evidence: CausalEvidenceClass::Static, confidence: 1.0, condition: None, timestamp_ms: None, metadata: BTreeMap::new() }
    }

    #[test]
    fn finds_interprocedural_data_flow_and_taint() {
        let engine = DeepCausalityEngine::from_facts(
            vec![entity("http.input", CausalEntityKind::Parameter), entity("handler", CausalEntityKind::Symbol), entity("db.write", CausalEntityKind::DataStore)],
            vec![fact("http.input", "handler", CausalRelationKind::Assigns), fact("handler", "db.write", CausalRelationKind::Writes)],
        );
        let p = engine.data_flow_path("http.input", "db.write", 8).unwrap();
        assert_eq!(p.entities, vec!["http.input", "handler", "db.write"]);
        assert_eq!(engine.taint_paths(&["http.input".into()], &["db.write".into()], 8).len(), 1);
    }

    #[test]
    fn sanitizer_prevents_taint_finding() {
        let mut sanitize = fact("input", "safe", CausalRelationKind::Sanitizes);
        sanitize.evidence = CausalEvidenceClass::Validation;
        let engine = DeepCausalityEngine::from_facts(
            vec![entity("input", CausalEntityKind::Parameter), entity("safe", CausalEntityKind::Value), entity("sink", CausalEntityKind::DataStore)],
            vec![sanitize, fact("safe", "sink", CausalRelationKind::Writes)],
        );
        assert!(engine.taint_paths(&["input".into()], &["sink".into()], 8).is_empty());
    }

    #[test]
    fn catches_constraint_conflicts() {
        let engine = DeepCausalityEngine::new();
        assert!(!engine.constraints_satisfiable(&["role==admin".into(), "role!=admin".into()]));
        assert!(engine.constraints_satisfiable(&["role==admin".into(), "active==true".into()]));
    }

    #[test]
    fn detects_unprotected_multi_writer() {
        let engine = DeepCausalityEngine::from_facts(
            vec![entity("a", CausalEntityKind::Symbol), entity("b", CausalEntityKind::Symbol), entity("balance", CausalEntityKind::DataStore)],
            vec![fact("a", "balance", CausalRelationKind::Writes), fact("b", "balance", CausalRelationKind::Writes)],
        );
        assert_eq!(engine.concurrency_hazards()[0].kind, "unprotected_multi_writer");
    }

    #[test]
    fn classifies_contract_breaks() {
        let engine = DeepCausalityEngine::new();
        let before = ApiContract { id: "v1".into(), fields: vec![ContractField { name: "email".into(), required: false, type_name: "string".into() }] };
        let after = ApiContract { id: "v2".into(), fields: vec![ContractField { name: "email".into(), required: true, type_name: "string".into() }] };
        assert_eq!(engine.compare_contracts(&before, &after)[0].classification, "breaking");
    }

    #[test]
    fn change_simulation_is_explicitly_predicted() {
        let engine = DeepCausalityEngine::from_facts(
            vec![entity("column", CausalEntityKind::Column), entity("handler", CausalEntityKind::Symbol), entity("test", CausalEntityKind::Test)],
            vec![fact("handler", "column", CausalRelationKind::Reads), fact("test", "handler", CausalRelationKind::Exercises)],
        );
        let result = engine.simulate_change(&[ChangeOperation { operation: "delete".into(), entity_id: "column".into(), replacement_id: None }], 4);
        assert_eq!(result.evidence, CausalEvidenceClass::Predicted);
        assert!(result.affected_entities.contains(&"handler".to_string()));
        assert!(result.affected_tests.contains(&"test".to_string()));
    }

    #[test]
    fn cross_repo_requires_real_repo_boundary() {
        let mut a = entity("a", CausalEntityKind::Service); a.repository = Some("repo-a".into());
        let mut b = entity("b", CausalEntityKind::Service); b.repository = Some("repo-b".into());
        let engine = DeepCausalityEngine::from_facts(vec![a, b], vec![fact("a", "b", CausalRelationKind::Calls)]);
        assert!(engine.cross_repo_path("a", "b", 3).is_some());
    }

    #[test]
    fn quality_metrics_are_graph_derived() {
        let engine = DeepCausalityEngine::from_facts(
            vec![entity("a", CausalEntityKind::Symbol), entity("b", CausalEntityKind::Symbol)],
            vec![fact("a", "b", CausalRelationKind::Calls)],
        );
        let metrics = engine.quality_metrics();
        assert_eq!(metrics.entities, 2);
        assert_eq!(metrics.relations, 1);
        assert_eq!(metrics.max_fan_out, 1);
    }
}
