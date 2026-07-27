//! Storage model definitions for serialization

use serde::{Serialize, Deserialize};
use crate::types::*;
use std::collections::HashMap;

/// Serializable representation of the full graph state
#[derive(Debug, Serialize, Deserialize)]
pub struct StoredGraph {
    pub nodes: Vec<StoredNode>,
    pub edges: Vec<StoredEdge>,
    pub metadata: GraphMetadata,
}

/// Serializable node for storage
#[derive(Debug, Serialize, Deserialize)]
pub struct StoredNode {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub exports: Vec<StoredSymbol>,
    pub imports: Vec<StoredImport>,
    pub metadata: HashMap<String, String>,
}

/// Serializable edge for storage
#[derive(Debug, Serialize, Deserialize)]
pub struct StoredEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: String,
    pub weight: f32,
    pub metadata: HashMap<String, String>,
}

/// Serializable symbol
#[derive(Debug, Serialize, Deserialize)]
pub struct StoredSymbol {
    pub name: String,
    pub kind: String,
    pub exported: bool,
    pub public: bool,
}

/// Serializable import
#[derive(Debug, Serialize, Deserialize)]
pub struct StoredImport {
    pub source: String,
    pub symbols: Vec<String>,
    pub kind: String,
}

/// Graph-level metadata
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphMetadata {
    pub total_files: usize,
    pub total_nodes: usize,
    pub total_edges: usize,
    pub languages: Vec<String>,
    pub scan_duration_ms: u64,
}
