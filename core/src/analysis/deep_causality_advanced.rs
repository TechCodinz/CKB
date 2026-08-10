//! Advanced V13.1 causal reasoning refinements.
//!
//! These routines deliberately build on the public evidence bundle rather than
//! introducing hidden facts. They strengthen sanitizer-aware taint traversal,
//! bounded symbolic ranges, and reverse failure propagation semantics.

use super::deep_causality::*;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

#[derive(Default, Clone)]
struct SymbolicState {
    exact_text: Option<String>,
    not_text: HashSet<String>,
    min: Option<(f64, bool)>,
    max: Option<(f64, bool)>,
}

impl DeepCausalityEngine {
    /// Sanitizer-aware interprocedural taint traversal. A path may cross a
    /// sanitizer/validator, but it is reported as a finding only if the sink is
    /// reachable without such evidence on that path.
    pub fn taint_paths_v2(&self, sources: &[String], sinks: &[String], max_depth: usize) -> Vec<TaintFinding> {
        let sink_set: HashSet<&str> = sinks.iter().map(String::as_str).collect();
        let mut findings = Vec::new();
        for source in sources {
            let mut queue = VecDeque::from([(
                source.clone(),
                vec![source.clone()],
                Vec::<CausalFact>::new(),
                false,
                false,
            )]);
            let mut seen: HashMap<(String, bool), usize> = HashMap::new();
            seen.insert((source.clone(), false), 0);

            while let Some((id, entities, facts, sanitized, crossed_boundary)) = queue.pop_front() {
                if facts.len() >= max_depth { continue; }
                for causal_fact in self.facts().iter().filter(|f| f.from == id && is_taint_flow_relation(&f.relation)) {
                    let next_sanitized = sanitized || matches!(causal_fact.relation, CausalRelationKind::Sanitizes | CausalRelationKind::Validates);
                    let next_boundary = crossed_boundary || causal_fact.relation == CausalRelationKind::TrustBoundary;
                    let next_depth = facts.len() + 1;
                    let state_key = (causal_fact.to.clone(), next_sanitized);
                    if seen.get(&state_key).map(|d| *d <= next_depth).unwrap_or(false) { continue; }
                    seen.insert(state_key, next_depth);

                    let mut next_entities = entities.clone();
                    next_entities.push(causal_fact.to.clone());
                    let mut next_facts = facts.clone();
                    next_facts.push(causal_fact.clone());

                    if sink_set.contains(causal_fact.to.as_str()) {
                        if !next_sanitized {
                            findings.push(TaintFinding {
                                source: source.clone(),
                                sink: causal_fact.to.clone(),
                                path: make_path(next_entities, next_facts),
                                crossed_trust_boundary: next_boundary,
                                sanitizer_observed: false,
                            });
                        }
                        continue;
                    }
                    queue.push_back((causal_fact.to.clone(), next_entities, next_facts, next_sanitized, next_boundary));
                }
            }
        }
        findings.sort_by(|a, b| (a.source.as_str(), a.sink.as_str(), a.path.entities.len()).cmp(&(b.source.as_str(), b.sink.as_str(), b.path.entities.len())));
        findings.dedup_by(|a, b| a.source == b.source && a.sink == b.sink && a.path.entities == b.path.entities);
        findings
    }

    /// Bounded symbolic constraints supporting equality, inequality, numeric
    /// ranges, and boolean/string literals. Unsupported expressions are not
    /// guessed; they remain opaque and cannot manufacture a contradiction.
    pub fn constraints_satisfiable_v2(&self, constraints: &[String]) -> bool {
        let mut vars: HashMap<String, SymbolicState> = HashMap::new();

        for raw in constraints {
            let Some((name, op, value)) = parse_constraint(raw) else { continue; };
            let state = vars.entry(name).or_default();
            match op.as_str() {
                "==" => {
                    if state.not_text.contains(&value) { return false; }
                    if state.exact_text.as_ref().map(|v| v != &value).unwrap_or(false) { return false; }
                    state.exact_text = Some(value.clone());
                    if let Ok(n) = value.parse::<f64>() {
                        if !numeric_in_bounds(n, state) { return false; }
                    }
                }
                "!=" => {
                    if state.exact_text.as_ref() == Some(&value) { return false; }
                    state.not_text.insert(value);
                }
                ">" | ">=" | "<" | "<=" => {
                    let Ok(n) = value.parse::<f64>() else { continue; };
                    match op.as_str() {
                        ">" => state.set_min(n, false),
                        ">=" => state.set_min(n, true),
                        "<" => state.set_max(n, false),
                        "<=" => state.set_max(n, true),
                        _ => unreachable!(),
                    }
                    if bounds_contradict(state) { return false; }
                    if let Some(exact) = state.exact_text.as_ref().and_then(|v| v.parse::<f64>().ok()) {
                        if !numeric_in_bounds(exact, state) { return false; }
                    }
                }
                _ => {}
            }
        }
        true
    }

    /// Propagate a failure from a dependency/resource back toward entities that
    /// rely on it. CKB graph relations such as `caller -> callee` and
    /// `service -> dependency` require reverse traversal for failure impact.
    /// Event delivery relations additionally allow event -> consumer traversal.
    pub fn failure_propagation_v2(&self, failed_entity: &str, max_depth: usize) -> Vec<String> {
        let mut seen = HashSet::from([failed_entity.to_string()]);
        let mut queue = VecDeque::from([(failed_entity.to_string(), 0usize)]);
        let mut affected = BTreeSet::new();

        while let Some((id, depth)) = queue.pop_front() {
            if depth >= max_depth { continue; }
            for causal_fact in self.facts() {
                let next = if causal_fact.to == id && reverse_failure_relation(&causal_fact.relation) {
                    Some(causal_fact.from.clone())
                } else if causal_fact.from == id && forward_failure_relation(&causal_fact.relation) {
                    Some(causal_fact.to.clone())
                } else {
                    None
                };
                let Some(next) = next else { continue; };
                if seen.insert(next.clone()) {
                    affected.insert(next.clone());
                    queue.push_back((next, depth + 1));
                }
            }
        }
        affected.into_iter().collect()
    }
}

fn is_taint_flow_relation(relation: &CausalRelationKind) -> bool {
    matches!(relation,
        CausalRelationKind::Assigns |
        CausalRelationKind::Derives |
        CausalRelationKind::Reads |
        CausalRelationKind::Writes |
        CausalRelationKind::Returns |
        CausalRelationKind::Calls |
        CausalRelationKind::RoutesTo |
        CausalRelationKind::Emits |
        CausalRelationKind::Consumes |
        CausalRelationKind::Publishes |
        CausalRelationKind::Subscribes |
        CausalRelationKind::Sanitizes |
        CausalRelationKind::Validates |
        CausalRelationKind::TrustBoundary
    )
}

fn reverse_failure_relation(relation: &CausalRelationKind) -> bool {
    matches!(relation,
        CausalRelationKind::Calls |
        CausalRelationKind::DependsOn |
        CausalRelationKind::Imports |
        CausalRelationKind::Reads |
        CausalRelationKind::ConnectsTo |
        CausalRelationKind::RoutesTo |
        CausalRelationKind::Subscribes |
        CausalRelationKind::WaitsFor |
        CausalRelationKind::Deploys
    )
}

fn forward_failure_relation(relation: &CausalRelationKind) -> bool {
    matches!(relation,
        CausalRelationKind::Consumes |
        CausalRelationKind::Retries |
        CausalRelationKind::TimesOut |
        CausalRelationKind::CircuitBreaks
    )
}

fn make_path(entities: Vec<String>, facts: Vec<CausalFact>) -> CausalPath {
    let minimum_confidence = facts.iter().map(|f| f.confidence).fold(1.0_f32, f32::min);
    let evidence_classes = facts.iter()
        .map(|f| format!("{:?}", f.evidence).to_ascii_lowercase())
        .collect();
    CausalPath { entities, facts, minimum_confidence, evidence_classes }
}

fn parse_constraint(raw: &str) -> Option<(String, String, String)> {
    let text = raw.trim();
    for op in [">=", "<=", "!=", "==", ">", "<"] {
        if let Some((left, right)) = text.split_once(op) {
            let name = left.trim();
            let value = right.trim();
            if !name.is_empty() && !value.is_empty() {
                return Some((name.to_string(), op.to_string(), value.trim_matches('"').to_string()));
            }
        }
    }
    None
}

trait BoundState {
    fn min_bound(&self) -> Option<(f64, bool)>;
    fn max_bound(&self) -> Option<(f64, bool)>;
    fn set_min(&mut self, value: f64, inclusive: bool);
    fn set_max(&mut self, value: f64, inclusive: bool);
}

impl BoundState for SymbolicState {
    fn min_bound(&self) -> Option<(f64, bool)> { self.min }
    fn max_bound(&self) -> Option<(f64, bool)> { self.max }
    fn set_min(&mut self, value: f64, inclusive: bool) {
        if self.min.map(|(v, i)| value > v || (value == v && !inclusive && i)).unwrap_or(true) {
            self.min = Some((value, inclusive));
        }
    }
    fn set_max(&mut self, value: f64, inclusive: bool) {
        if self.max.map(|(v, i)| value < v || (value == v && !inclusive && i)).unwrap_or(true) {
            self.max = Some((value, inclusive));
        }
    }
}

fn bounds_contradict<T: BoundState>(state: &T) -> bool {
    match (state.min_bound(), state.max_bound()) {
        (Some((min, min_inc)), Some((max, max_inc))) => min > max || (min == max && (!min_inc || !max_inc)),
        _ => false,
    }
}
fn numeric_in_bounds<T: BoundState>(n: f64, state: &T) -> bool {
    if let Some((min, inclusive)) = state.min_bound() {
        if n < min || (n == min && !inclusive) { return false; }
    }
    if let Some((max, inclusive)) = state.max_bound() {
        if n > max || (n == max && !inclusive) { return false; }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn entity(id: &str, kind: CausalEntityKind) -> CausalEntity {
        CausalEntity { id:id.into(), kind, name:id.into(), repository:None, path:None, attributes:BTreeMap::new() }
    }
    fn fact(from:&str,to:&str,relation:CausalRelationKind)->CausalFact{
        CausalFact{from:from.into(),to:to.into(),relation,evidence:CausalEvidenceClass::Static,confidence:1.0,condition:None,timestamp_ms:None,metadata:BTreeMap::new()}
    }

    #[test]
    fn taint_reports_only_unsanitized_path() {
        let engine=DeepCausalityEngine::from_facts(
            vec![entity("input",CausalEntityKind::Parameter),entity("raw",CausalEntityKind::Value),entity("san",CausalEntityKind::Symbol),entity("sink",CausalEntityKind::RuntimeResource)],
            vec![fact("input","raw",CausalRelationKind::Assigns),fact("raw","sink",CausalRelationKind::Writes),fact("raw","san",CausalRelationKind::Sanitizes),fact("san","sink",CausalRelationKind::Writes)]
        );
        let findings=engine.taint_paths_v2(&["input".into()], &["sink".into()], 8);
        assert_eq!(findings.len(),1);
        assert!(findings[0].path.entities.contains(&"raw".into()));
    }

    #[test]
    fn sanitized_only_path_is_not_a_finding() {
        let engine=DeepCausalityEngine::from_facts(
            vec![entity("input",CausalEntityKind::Parameter),entity("san",CausalEntityKind::Symbol),entity("sink",CausalEntityKind::RuntimeResource)],
            vec![fact("input","san",CausalRelationKind::Sanitizes),fact("san","sink",CausalRelationKind::Writes)]
        );
        assert!(engine.taint_paths_v2(&["input".into()], &["sink".into()], 8).is_empty());
    }

    #[test]
    fn numeric_constraints_detect_empty_ranges() {
        let engine=DeepCausalityEngine::new();
        assert!(!engine.constraints_satisfiable_v2(&["age>=18".into(),"age<18".into()]));
        assert!(engine.constraints_satisfiable_v2(&["age>=18".into(),"age<65".into(),"active==true".into()]));
        assert!(!engine.constraints_satisfiable_v2(&["age>=18".into(),"age==17".into()]));
    }

    #[test]
    fn failure_flows_from_dependency_back_to_callers() {
        let engine=DeepCausalityEngine::from_facts(
            vec![entity("api",CausalEntityKind::Api),entity("service",CausalEntityKind::Service),entity("db",CausalEntityKind::DataStore)],
            vec![fact("api","service",CausalRelationKind::Calls),fact("service","db",CausalRelationKind::DependsOn)]
        );
        let affected=engine.failure_propagation_v2("db",8);
        assert!(affected.contains(&"service".into()));
        assert!(affected.contains(&"api".into()));
    }
}
