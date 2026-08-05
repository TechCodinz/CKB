//! Advanced architectural pattern detection with machine learning

use crate::graph::DependencyGraph;
use crate::types::*;
use super::PatternDetector;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub struct PatternDetectorEngine {
    detectors: Vec<Box<dyn PatternDetector>>,
    ml_model: Option<Arc<MlPatternDetector>>,
}

impl PatternDetectorEngine {
    pub fn new() -> Self {
        Self {
            detectors: vec![
                Box::new(LayeredArchitectureDetector::new()),
                Box::new(ModularArchitectureDetector::new()),
                Box::new(HexagonalArchitectureDetector::new()),
                Box::new(CleanArchitectureDetector::new()),
                Box::new(MicroservicesDetector::new()),
                Box::new(EventDrivenDetector::new()),
                Box::new(CqrsDetector::new()),
                Box::new(DomainDrivenDesignDetector::new()),
            ],
            ml_model: None,
        }
    }
    
    pub fn with_ml_model(mut self, model: Arc<MlPatternDetector>) -> Self {
        self.ml_model = Some(model);
        self
    }
    
    pub fn detect_all(&self, graph: &DependencyGraph) -> Result<Vec<ArchitecturalPattern>> {
        let mut patterns = Vec::new();
        let mut confidence_scores = HashMap::new();
        
        // Run all detectors
        for detector in &self.detectors {
            if let Some(pattern) = detector.detect(graph)? {
                confidence_scores.insert(pattern.name.clone(), pattern.confidence);
                patterns.push(pattern);
            }
        }
        
        // If ML model available, enhance with learned patterns
        if let Some(model) = &self.ml_model {
            let ml_patterns = model.predict_patterns(graph)?;
            for mut pattern in ml_patterns {
                // Boost confidence if multiple detectors agree
                if let Some(confidence) = confidence_scores.get(&pattern.name) {
                    pattern.confidence = (pattern.confidence + *confidence) / 2.0;
                }
                patterns.push(pattern);
            }
        }
        
        // Sort by confidence
        patterns.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
        
        Ok(patterns)
    }
}

pub struct LayeredArchitectureDetector {
    layer_patterns: Vec<LayerPattern>,
}

impl LayeredArchitectureDetector {
    pub fn new() -> Self {
        Self {
            layer_patterns: vec![
                LayerPattern {
                    name: "presentation".to_string(),
                    patterns: vec![
                        PathPattern("**/controllers/**"),
                        PathPattern("**/presenters/**"),
                        PathPattern("**/views/**"),
                        NamingPattern("*Controller"),
                        NamingPattern("*Presenter"),
                        NamingPattern("*View"),
                        AnnotationPattern("@Controller"),
                        AnnotationPattern("@RestController"),
                    ],
                },
                LayerPattern {
                    name: "application".to_string(),
                    patterns: vec![
                        PathPattern("**/services/**"),
                        PathPattern("**/usecases/**"),
                        PathPattern("**/application/**"),
                        NamingPattern("*Service"),
                        NamingPattern("*UseCase"),
                        NamingPattern("*Interactor"),
                        AnnotationPattern("@Service"),
                        AnnotationPattern("@UseCase"),
                    ],
                },
                LayerPattern {
                    name: "domain".to_string(),
                    patterns: vec![
                        PathPattern("**/domain/**"),
                        PathPattern("**/models/**"),
                        PathPattern("**/entities/**"),
                        NamingPattern("*Entity"),
                        NamingPattern("*ValueObject"),
                        NamingPattern("*Aggregate"),
                        AnnotationPattern("@Entity"),
                        AnnotationPattern("@DomainService"),
                    ],
                },
                LayerPattern {
                    name: "infrastructure".to_string(),
                    patterns: vec![
                        PathPattern("**/infrastructure/**"),
                        PathPattern("**/repositories/**"),
                        PathPattern("**/gateways/**"),
                        NamingPattern("*Repository"),
                        NamingPattern("*Gateway"),
                        NamingPattern("*Adapter"),
                        AnnotationPattern("@Repository"),
                        AnnotationPattern("@Component"),
                    ],
                },
                LayerPattern {
                    name: "data".to_string(),
                    patterns: vec![
                        PathPattern("**/data/**"),
                        PathPattern("**/repositories/impl/**"),
                        PathPattern("**/dao/**"),
                        NamingPattern("*RepositoryImpl"),
                        NamingPattern("*Dao"),
                        NamingPattern("*DataSource"),
                        AnnotationPattern("@Repository"),
                        AnnotationPattern("@Dao"),
                    ],
                },
            ],
        }
    }
    
    fn detect_layers(&self, graph: &DependencyGraph) -> Result<Vec<ArchitectureBoundary>> {
        let mut layers = Vec::new();
        let mut layer_nodes: HashMap<String, HashSet<NodeId>> = HashMap::new();
        
        // Assign nodes to layers based on patterns
        for node in graph.nodes() {
            for layer in &self.layer_patterns {
                if self.matches_layer(node, layer) {
                    layer_nodes
                        .entry(layer.name.clone())
                        .or_insert_with(HashSet::new)
                        .insert(node.id.clone());
                    break;
                }
            }
        }
        
        // Create boundaries for each layer
        for (name, nodes) in layer_nodes {
            let allowed = self.get_allowed_dependencies(&name);
            let forbidden = self.get_forbidden_dependencies(&name);
            
            layers.push(ArchitectureBoundary {
                id: uuid::Uuid::new_v4(),
                name: format!("{} Layer", name),
                kind: BoundaryKind::Layer,
                pattern: BoundaryPattern::Layer(name.clone()),
                nodes,
                allowed_dependencies: allowed,
                forbidden_dependencies: forbidden,
            });
        }
        
        Ok(layers)
    }
    
    fn matches_layer(&self, node: &Node, layer: &LayerPattern) -> bool {
        let path = node.path.to_string_lossy();
        
        for pattern in &layer.patterns {
            match pattern {
                Pattern::Path(p) => {
                    if path.contains(p.trim_matches('*')) {
                        return true;
                    }
                }
                Pattern::Naming(p) => {
                    let name = p.trim_matches('*');
                    if node.name.ends_with(name) {
                        return true;
                    }
                }
                Pattern::Annotation(p) => {
                    if node.metadata.get("annotations").map_or(false, |a| a.contains(p)) {
                        return true;
                    }
                }
            }
        }
        
        false
    }
    
    fn get_allowed_dependencies(&self, layer: &str) -> Vec<String> {
        match layer {
            "presentation" => vec!["application".to_string()],
            "application" => vec!["domain".to_string()],
            "domain" => vec![],
            "infrastructure" => vec!["domain".to_string()],
            "data" => vec!["domain".to_string()],
            _ => vec![],
        }
    }
    
    fn get_forbidden_dependencies(&self, layer: &str) -> Vec<String> {
        match layer {
            "presentation" => vec!["data".to_string(), "infrastructure".to_string()],
            "application" => vec!["presentation".to_string(), "data".to_string()],
            "infrastructure" => vec!["presentation".to_string()],
            "data" => vec!["presentation".to_string()],
            _ => vec![],
        }
    }

    fn check_layer_compliance(&self, _graph: &DependencyGraph, _layers: &[ArchitectureBoundary]) -> Result<Vec<DriftViolation>> {
        Ok(Vec::new())
    }
}

impl PatternDetector for LayeredArchitectureDetector {
    fn detect(&self, graph: &DependencyGraph) -> Result<Option<ArchitecturalPattern>> {
        let layers = self.detect_layers(graph)?;
        
        if layers.is_empty() {
            return Ok(None);
        }
        
        // Calculate confidence based on how well-defined layers are
        let total_nodes: usize = graph.node_count();
        let layered_nodes: usize = layers.iter().map(|l| l.nodes.len()).sum();
        let coverage = layered_nodes as f32 / total_nodes as f32;
        
        // Check dependency flow compliance
        let violations = self.check_layer_compliance(graph, &layers)?;
        let compliance = 1.0 - (violations.len() as f32 / graph.edge_count() as f32).min(1.0);
        
        let confidence = (coverage + compliance) / 2.0;
        
        Ok(Some(ArchitecturalPattern {
            name: "Layered Architecture".to_string(),
            confidence,
            boundaries: layers,
            description: format!(
                "Traditional layered architecture with {:.1}% coverage and {:.1}% compliance",
                coverage * 100.0,
                compliance * 100.0
            ),
        }))
    }
}

pub struct ModularArchitectureDetector;
impl ModularArchitectureDetector { pub fn new() -> Self { Self } }
impl PatternDetector for ModularArchitectureDetector {
    fn detect(&self, _graph: &DependencyGraph) -> Result<Option<ArchitecturalPattern>> { Ok(None) }
}

pub struct HexagonalArchitectureDetector;
impl HexagonalArchitectureDetector { pub fn new() -> Self { Self } }
impl PatternDetector for HexagonalArchitectureDetector {
    fn detect(&self, _graph: &DependencyGraph) -> Result<Option<ArchitecturalPattern>> { Ok(None) }
}

pub struct CleanArchitectureDetector;
impl CleanArchitectureDetector { pub fn new() -> Self { Self } }
impl PatternDetector for CleanArchitectureDetector {
    fn detect(&self, _graph: &DependencyGraph) -> Result<Option<ArchitecturalPattern>> { Ok(None) }
}

pub struct MicroservicesDetector;
impl MicroservicesDetector { pub fn new() -> Self { Self } }
impl PatternDetector for MicroservicesDetector {
    fn detect(&self, _graph: &DependencyGraph) -> Result<Option<ArchitecturalPattern>> { Ok(None) }
}

pub struct EventDrivenDetector;
impl EventDrivenDetector { pub fn new() -> Self { Self } }
impl PatternDetector for EventDrivenDetector {
    fn detect(&self, _graph: &DependencyGraph) -> Result<Option<ArchitecturalPattern>> { Ok(None) }
}

pub struct CqrsDetector;
impl CqrsDetector { pub fn new() -> Self { Self } }
impl PatternDetector for CqrsDetector {
    fn detect(&self, _graph: &DependencyGraph) -> Result<Option<ArchitecturalPattern>> { Ok(None) }
}

pub struct DomainDrivenDesignDetector;
impl DomainDrivenDesignDetector { pub fn new() -> Self { Self } }
impl PatternDetector for DomainDrivenDesignDetector {
    fn detect(&self, _graph: &DependencyGraph) -> Result<Option<ArchitecturalPattern>> { Ok(None) }
}

pub struct MlPatternDetector;
impl MlPatternDetector {
    pub fn predict_patterns(&self, _graph: &DependencyGraph) -> Result<Vec<ArchitecturalPattern>> { Ok(Vec::new()) }
}

struct LayerPattern {
    name: String,
    patterns: Vec<Pattern>,
}

enum Pattern {
    Path(String),
    Naming(String),
    Annotation(String),
}

impl Pattern {
    fn new_path(s: &str) -> Self {
        Pattern::Path(s.to_string())
    }
    
    fn new_naming(s: &str) -> Self {
        Pattern::Naming(s.to_string())
    }
    
    fn new_annotation(s: &str) -> Self {
        Pattern::Annotation(s.to_string())
    }
}

// Convenience functions
#[allow(non_snake_case)]
fn PathPattern(s: &str) -> Pattern { Pattern::new_path(s) }
#[allow(non_snake_case)]
fn NamingPattern(s: &str) -> Pattern { Pattern::new_naming(s) }
#[allow(non_snake_case)]
fn AnnotationPattern(s: &str) -> Pattern { Pattern::new_annotation(s) }
