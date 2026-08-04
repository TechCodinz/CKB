//! Core data types used throughout CKB

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use uuid::Uuid;

/// Unique identifier for a node in the dependency graph
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeId(pub String);

/// A node in the dependency graph (file, class, function, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub name: String,
    pub path: PathBuf,
    pub line: u32,
    pub column: u32,
    pub exports: Vec<Symbol>,
    pub imports: Vec<Import>,
    pub metadata: HashMap<String, String>,
}

/// Kind of node in the graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    File,
    Module,
    Namespace,
    Class,
    Interface,
    Enum,
    Function,
    Method,
    Variable,
    Type,
}

/// A symbol exported from a file/module
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub exported: bool,
    pub public: bool,
}

/// Kind of symbol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Class,
    Interface,
    Function,
    Variable,
    Type,
    Constant,
}

/// An import statement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub source: String,
    pub symbols: Vec<String>,
    pub kind: ImportKind,
}

/// Kind of import
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportKind {
    Default,
    Named,
    Namespace,
    Type,
}

/// Edge in the dependency graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: Uuid,
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub weight: f32,
    pub metadata: HashMap<String, String>,
}

/// Kind of dependency edge
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    Import,
    Extends,
    Implements,
    Calls,
    Instantiates,
    Returns,
    Parameter,
    Property,
}

/// Architecture boundary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureBoundary {
    pub id: Uuid,
    pub name: String,
    pub kind: BoundaryKind,
    pub pattern: BoundaryPattern,
    pub nodes: HashSet<NodeId>,
    pub allowed_dependencies: Vec<String>,
    pub forbidden_dependencies: Vec<String>,
}

/// Kind of boundary
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoundaryKind {
    Layer,
    Module,
    Domain,
    Component,
}

/// Pattern for boundary detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BoundaryPattern {
    PathPattern(String),           // e.g., "src/domain/**"
    NamingPattern(String),          // e.g., "*Service", "*Controller"
    AnnotationPattern(String),      // e.g., "@Injectable", "@Component"
    ConventionPattern(String),      // e.g., "Clean Architecture", "DDD"
    Layer(String),
}

/// Architectural pattern detected in codebase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitecturalPattern {
    pub name: String,
    pub confidence: f32,
    pub boundaries: Vec<ArchitectureBoundary>,
    pub description: String,
}

/// Drift violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftViolation {
    pub id: Uuid,
    pub kind: ViolationKind,
    pub from: NodeId,
    pub to: NodeId,
    pub boundary: String,
    pub message: String,
    pub severity: Severity,
    pub suggested_fix: Option<String>,
}

/// Kind of violation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ViolationKind {
    ForbiddenDependency,
    CircularDependency,
    LayerSkip,
    BoundaryCrossing,
    GodObject,
    UnstableDependency,
}

/// Severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

/// Type of change for impact analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeType {
    Add,
    Modify,
    Delete,
    Rename,
}


/// Impact analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactAnalysis {
    pub direct_impacts: Vec<ImpactedNode>,
    pub indirect_impacts: Vec<ImpactedNode>,
    pub risk_score: f32,
    pub estimated_effort: String,
}

/// Node impacted by a change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactedNode {
    pub node: NodeId,
    pub impact_kind: ImpactKind,
    pub confidence: f32,
    pub path: PathBuf,
    pub line: u32,
}

/// A single change within a multi-edit session (see `SessionImpactSummary`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionChange {
    pub file: String,
    pub line: u32,
    pub change_type: ChangeType,
}

/// Aggregated blast-radius view across an entire editing session (e.g. every
/// file an AI coding agent touched in one pass), instead of one
/// `ImpactAnalysis` per edit that nobody has time to read individually.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionImpactSummary {
    pub changes_analyzed: usize,
    pub unique_affected_nodes: usize,
    pub unique_affected_files: usize,
    pub affected_files: Vec<String>,
    pub highest_risk_score: f32,
    pub average_risk_score: f32,
    /// Affected node IDs (`"path::function"`) that have zero test coverage,
    /// per the real `TestCoverageAnalyzer` — i.e. "this session touched code
    /// with no tests protecting it."
    pub untested_affected_nodes: Vec<String>,
    pub per_change: Vec<ImpactAnalysis>,
}

/// Kind of impact
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImpactKind {
    CompileBreak,
    RuntimeBreak,
    TypeViolation,
    Behavioral,
    Unknown,
}

/// A function/method call site detected by AST parser
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub caller_name: String,
    pub callee_name: String,
    pub line: u32,
    pub column: u32,
}

/// A type inheritance or interface implementation relationship
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeRelation {
    pub source_type: String,
    pub target_type: String,
    pub kind: TypeRelationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeRelationKind {
    Extends,
    Implements,
}

/// Dynamic runtime execution metrics ingested from runtime telemetry / OpenTelemetry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeMetrics {
    pub execution_count: u64,
    pub avg_latency_ms: f32,
    pub error_rate: f32,
    pub is_hotpath: bool,
}

/// A dynamic runtime call trace between nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicTrace {
    pub caller_node: NodeId,
    pub callee_node: NodeId,
    pub invocation_count: u64,
    pub last_seen_timestamp: u64,
}

