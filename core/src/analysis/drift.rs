//! Advanced drift detection with real-time analysis

use crate::graph::DependencyGraph;
use crate::types::*;
use anyhow::Result;
use std::collections::{HashSet, VecDeque, HashMap};

pub struct DriftDetector {
    rules: Vec<ArchitectureRule>,
    severity_levels: HashMap<ViolationKind, Severity>,
}

impl DriftDetector {
    pub fn new() -> Self {
        let mut severity_levels = HashMap::new();
        severity_levels.insert(ViolationKind::ForbiddenDependency, Severity::Error);
        severity_levels.insert(ViolationKind::CircularDependency, Severity::Critical);
        severity_levels.insert(ViolationKind::LayerSkip, Severity::Warning);
        severity_levels.insert(ViolationKind::BoundaryCrossing, Severity::Error);
        severity_levels.insert(ViolationKind::GodObject, Severity::Warning);
        severity_levels.insert(ViolationKind::UnstableDependency, Severity::Info);
        
        Self {
            rules: vec![
                ArchitectureRule::new(Box::new(ForbiddenDependencyRule)),
                ArchitectureRule::new(Box::new(CircularDependencyRule)),
                ArchitectureRule::new(Box::new(LayerSkipRule)),
                ArchitectureRule::new(Box::new(GodObjectRule)),
                ArchitectureRule::new(Box::new(StabilityRule)),
            ],
            severity_levels,
        }
    }
    
    pub fn detect_all(&self, graph: &DependencyGraph, boundaries: &[ArchitectureBoundary]) -> Result<Vec<DriftViolation>> {
        let mut all_violations = Vec::new();
        
        for rule in &self.rules {
            let violations = rule.check(graph, boundaries)?;
            all_violations.extend(violations);
        }
        
        // Assign severity levels
        for violation in &mut all_violations {
            violation.severity = *self.severity_levels
                .get(&violation.kind)
                .unwrap_or(&Severity::Info);
        }
        
        // Sort by severity
        all_violations.sort_by(|a, b| b.severity.cmp(&a.severity));
        
        Ok(all_violations)
    }
    
    pub fn detect_incremental(&self, 
                              graph: &DependencyGraph,
                              boundaries: &[ArchitectureBoundary],
                              changed_files: &[String]) -> Result<Vec<DriftViolation>> {
        // Only analyze subgraph affected by changes
        let mut affected_nodes = HashSet::new();
        let mut queue = VecDeque::new();
        
        // Find changed nodes
        for file in changed_files {
            if let Some(node) = graph.find_node_by_path(file) {
                affected_nodes.insert(node.id.clone());
                queue.push_back(node.id.clone());
            }
        }
        
        // BFS to find affected area
        while let Some(node_id) = queue.pop_front() {
            let deps = graph.get_dependencies(&node_id)?;
            for dep in deps {
                if !affected_nodes.contains(&dep) {
                    affected_nodes.insert(dep.clone());
                    queue.push_back(dep);
                }
            }
        }
        
        // Check only affected subgraph
        let subgraph = graph.extract_subgraph(&affected_nodes)?;
        self.detect_all(&subgraph, boundaries)
    }
}

trait Rule: Send + Sync {
    fn check(&self, graph: &DependencyGraph, boundaries: &[ArchitectureBoundary]) -> Result<Vec<DriftViolation>>;
}

struct ArchitectureRule {
    rule: Box<dyn Rule>,
}

impl ArchitectureRule {
    fn new(rule: Box<dyn Rule>) -> Self {
        Self { rule }
    }
    
    fn check(&self, graph: &DependencyGraph, boundaries: &[ArchitectureBoundary]) -> Result<Vec<DriftViolation>> {
        self.rule.check(graph, boundaries)
    }
}

struct ForbiddenDependencyRule;

impl Rule for ForbiddenDependencyRule {
    fn check(&self, graph: &DependencyGraph, boundaries: &[ArchitectureBoundary]) -> Result<Vec<DriftViolation>> {
        let mut violations = Vec::new();
        
        for edge in graph.edges() {
            for boundary in boundaries {
                if boundary.forbidden_dependencies.is_empty() {
                    continue;
                }
                
                let from_in = boundary.nodes.contains(&edge.from);
                let to_in = boundary.nodes.contains(&edge.to);
                
                if from_in && !to_in {
                    // Check if this dependency is forbidden
                    for forbidden in &boundary.forbidden_dependencies {
                        if edge.to.0.contains(forbidden) {
                            violations.push(DriftViolation {
                                id: uuid::Uuid::new_v4(),
                                kind: ViolationKind::ForbiddenDependency,
                                from: edge.from.clone(),
                                to: edge.to.clone(),
                                boundary: boundary.name.clone(),
                                message: format!(
                                    "{} should not depend on {} (violates {} boundary)",
                                    edge.from.0, edge.to.0, boundary.name
                                ),
                                severity: Severity::Error,
                                suggested_fix: Some(format!(
                                    "Move dependency through allowed layer or refactor to respect boundaries"
                                )),
                            });
                        }
                    }
                }
            }
        }
        
        Ok(violations)
    }
}

struct CircularDependencyRule;

impl Rule for CircularDependencyRule {
    fn check(&self, graph: &DependencyGraph, _boundaries: &[ArchitectureBoundary]) -> Result<Vec<DriftViolation>> {
        let mut violations = Vec::new();
        
        // Find cycles in graph
        let cycles = graph.find_cycles()?;
        
        for cycle in cycles {
            violations.push(DriftViolation {
                id: uuid::Uuid::new_v4(),
                kind: ViolationKind::CircularDependency,
                from: cycle[0].clone(),
                to: cycle.last().cloned().unwrap_or_else(|| cycle[0].clone()),
                boundary: "global".to_string(),
                message: format!("Circular dependency detected: {}", 
                    cycle.iter().map(|id| id.0.clone()).collect::<Vec<_>>().join(" -> ")),
                severity: Severity::Critical,
                suggested_fix: Some("Break the cycle by extracting interface or using dependency inversion".to_string()),
            });
        }
        
        Ok(violations)
    }
}

struct LayerSkipRule;

impl Rule for LayerSkipRule {
    fn check(&self, graph: &DependencyGraph, boundaries: &[ArchitectureBoundary]) -> Result<Vec<DriftViolation>> {
        let mut violations = Vec::new();
        
        // Group boundaries by type
        let layers: Vec<_> = boundaries.iter()
            .filter(|b| b.kind == BoundaryKind::Layer)
            .collect();
        
        if layers.len() < 2 {
            return Ok(violations);
        }
        
        // Sort layers by supposed order
        let layer_order: Vec<_> = layers.iter()
            .map(|l| l.name.clone())
            .collect();
        
        for edge in graph.edges() {
            let from_layer = layers.iter().find(|l| l.nodes.contains(&edge.from));
            let to_layer = layers.iter().find(|l| l.nodes.contains(&edge.to));
            
            if let (Some(from), Some(to)) = (from_layer, to_layer) {
                let from_idx = layer_order.iter().position(|n| n == &from.name);
                let to_idx = layer_order.iter().position(|n| n == &to.name);
                
                if let (Some(fi), Some(ti)) = (from_idx, to_idx) {
                    if ti > fi + 1 {
                        violations.push(DriftViolation {
                            id: uuid::Uuid::new_v4(),
                            kind: ViolationKind::LayerSkip,
                            from: edge.from.clone(),
                            to: edge.to.clone(),
                            boundary: from.name.clone(),
                            message: format!(
                                "{} (in {}) skips layer(s) to depend on {} (in {})",
                                edge.from.0, from.name, edge.to.0, to.name
                            ),
                            severity: Severity::Warning,
                            suggested_fix: Some(format!(
                                "Depend on {} layer instead, or add interface in middle layer",
                                layers[fi + 1].name
                            )),
                        });
                    }
                }
            }
        }
        
        Ok(violations)
    }
}

struct GodObjectRule;

impl Rule for GodObjectRule {
    fn check(&self, graph: &DependencyGraph, _boundaries: &[ArchitectureBoundary]) -> Result<Vec<DriftViolation>> {
        let mut violations = Vec::new();
        
        // Find nodes with too many dependencies
        for node in graph.nodes() {
            let incoming = graph.incoming_degree(&node.id)?;
            let outgoing = graph.outgoing_degree(&node.id)?;
            
            if incoming > 20 || outgoing > 20 {
                violations.push(DriftViolation {
                    id: uuid::Uuid::new_v4(),
                    kind: ViolationKind::GodObject,
                    from: node.id.clone(),
                    to: node.id.clone(),
                    boundary: "global".to_string(),
                    message: format!(
                        "{} has {} incoming and {} outgoing dependencies - consider splitting",
                        node.name, incoming, outgoing
                    ),
                    severity: Severity::Warning,
                    suggested_fix: Some("Extract related functionality into separate modules".to_string()),
                });
            }
        }
        
        Ok(violations)
    }
}

struct StabilityRule;

impl Rule for StabilityRule {
    fn check(&self, graph: &DependencyGraph, _boundaries: &[ArchitectureBoundary]) -> Result<Vec<DriftViolation>> {
        let mut violations = Vec::new();
        
        // Calculate stability metrics (fan-in / (fan-in + fan-out))
        for node in graph.nodes() {
            let incoming = graph.incoming_degree(&node.id)? as f32;
            let outgoing = graph.outgoing_degree(&node.id)? as f32;
            
            if incoming + outgoing > 0.0 {
                let stability = outgoing / (incoming + outgoing);
                
                // Unstable components should not be depended on by many
                if stability > 0.7 && incoming > 5.0 {
                    violations.push(DriftViolation {
                        id: uuid::Uuid::new_v4(),
                        kind: ViolationKind::UnstableDependency,
                        from: node.id.clone(),
                        to: node.id.clone(),
                        boundary: "global".to_string(),
                        message: format!(
                            "{} is unstable (stability={:.2}) but has {} dependents",
                            node.name, stability, incoming
                        ),
                        severity: Severity::Info,
                        suggested_fix: Some("Stabilize interface or invert dependencies".to_string()),
                    });
                }
            }
        }
        
        Ok(violations)
    }
}
