//! Boundary inference implementations

use crate::graph::DependencyGraph;
use crate::types::*;
use crate::analysis::BoundaryInferencer;
use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// Infers architectural boundaries based on file paths
pub struct PathBasedInferencer;

impl BoundaryInferencer for PathBasedInferencer {
    fn infer(&self, graph: &DependencyGraph) -> Result<Vec<ArchitectureBoundary>> {
        let mut boundaries = Vec::new();
        let mut path_groups: HashMap<String, HashSet<NodeId>> = HashMap::new();

        // Group nodes by their top-level directory
        for node in graph.nodes() {
            let path = node.path.to_string_lossy();
            let parts: Vec<&str> = path.split(['/', '\\']).collect();

            if parts.len() >= 2 {
                // Use the first meaningful directory as the boundary
                let group = parts.iter()
                    .find(|p| !p.is_empty() && *p != &"src" && *p != &".")
                    .unwrap_or(&"root")
                    .to_string();

                path_groups
                    .entry(group)
                    .or_insert_with(HashSet::new)
                    .insert(node.id.clone());
            }
        }

        for (name, nodes) in path_groups {
            if nodes.len() > 1 {
                boundaries.push(ArchitectureBoundary {
                    id: uuid::Uuid::new_v4(),
                    name: format!("{} module", name),
                    kind: BoundaryKind::Module,
                    pattern: BoundaryPattern::PathPattern(format!("**/{name}/**")),
                    nodes,
                    allowed_dependencies: vec![],
                    forbidden_dependencies: vec![],
                });
            }
        }

        Ok(boundaries)
    }
}

/// Infers boundaries based on naming conventions
pub struct NamingBasedInferencer;

impl BoundaryInferencer for NamingBasedInferencer {
    fn infer(&self, graph: &DependencyGraph) -> Result<Vec<ArchitectureBoundary>> {
        let mut boundaries = Vec::new();
        let mut naming_groups: HashMap<String, HashSet<NodeId>> = HashMap::new();

        let suffixes = [
            ("Controller", "presentation"),
            ("Service", "application"),
            ("Repository", "data"),
            ("Entity", "domain"),
            ("Model", "domain"),
            ("Handler", "presentation"),
            ("Middleware", "infrastructure"),
            ("Gateway", "infrastructure"),
            ("Adapter", "infrastructure"),
        ];

        for node in graph.nodes() {
            for (suffix, layer) in &suffixes {
                if node.name.ends_with(suffix) {
                    naming_groups
                        .entry(layer.to_string())
                        .or_insert_with(HashSet::new)
                        .insert(node.id.clone());
                    break;
                }
            }
        }

        for (name, nodes) in naming_groups {
            if !nodes.is_empty() {
                boundaries.push(ArchitectureBoundary {
                    id: uuid::Uuid::new_v4(),
                    name: format!("{} layer", name),
                    kind: BoundaryKind::Layer,
                    pattern: BoundaryPattern::NamingPattern(format!("*{name}")),
                    nodes,
                    allowed_dependencies: vec![],
                    forbidden_dependencies: vec![],
                });
            }
        }

        Ok(boundaries)
    }
}

/// Infers boundaries based on annotations/decorators
pub struct AnnotationBasedInferencer;

impl BoundaryInferencer for AnnotationBasedInferencer {
    fn infer(&self, graph: &DependencyGraph) -> Result<Vec<ArchitectureBoundary>> {
        let mut boundaries = Vec::new();
        let mut annotation_groups: HashMap<String, HashSet<NodeId>> = HashMap::new();

        let annotation_mapping = [
            ("@Controller", "presentation"),
            ("@RestController", "presentation"),
            ("@Service", "application"),
            ("@Repository", "data"),
            ("@Entity", "domain"),
            ("@Component", "infrastructure"),
            ("@Injectable", "application"),
        ];

        for node in graph.nodes() {
            if let Some(annotations) = node.metadata.get("annotations") {
                for (annotation, layer) in &annotation_mapping {
                    if annotations.contains(annotation) {
                        annotation_groups
                            .entry(layer.to_string())
                            .or_insert_with(HashSet::new)
                            .insert(node.id.clone());
                        break;
                    }
                }
            }
        }

        for (name, nodes) in annotation_groups {
            if !nodes.is_empty() {
                boundaries.push(ArchitectureBoundary {
                    id: uuid::Uuid::new_v4(),
                    name: format!("{} layer (annotation)", name),
                    kind: BoundaryKind::Layer,
                    pattern: BoundaryPattern::AnnotationPattern(format!("@{name}")),
                    nodes,
                    allowed_dependencies: vec![],
                    forbidden_dependencies: vec![],
                });
            }
        }

        Ok(boundaries)
    }
}
