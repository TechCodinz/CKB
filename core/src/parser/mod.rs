//! Multi-language parser infrastructure

mod typescript;
mod python;
mod go;
mod rust;
mod java;

use std::path::Path;
use std::collections::HashMap;
use anyhow::Result;
use crate::types::{Node, Symbol, Import, FunctionCall, TypeRelation};

pub struct LanguageParser {
    parsers: HashMap<String, Box<dyn LanguageParserTrait + Send + Sync>>,
}

impl LanguageParser {
    pub fn new() -> Self {
        let mut parsers: HashMap<String, Box<dyn LanguageParserTrait + Send + Sync>> = HashMap::new();

        // Register every extension that the Reality discovery layer advertises.
        // TSX uses the TypeScript parser; JSX/MJS use the JavaScript parser.
        // This prevents discovery from accepting a file only for parse_file()
        // to silently reject it later as an unsupported extension.
        parsers.insert("ts".to_string(), Box::new(typescript::TypeScriptParser::new()));
        parsers.insert("tsx".to_string(), Box::new(typescript::TypeScriptParser::new()));
        parsers.insert("js".to_string(), Box::new(typescript::JavaScriptParser::new()));
        parsers.insert("jsx".to_string(), Box::new(typescript::JavaScriptParser::new()));
        parsers.insert("mjs".to_string(), Box::new(typescript::JavaScriptParser::new()));
        parsers.insert("py".to_string(), Box::new(python::PythonParser::new()));
        parsers.insert("go".to_string(), Box::new(go::GoParser::new()));
        parsers.insert("rs".to_string(), Box::new(rust::RustParser::new()));
        parsers.insert("java".to_string(), Box::new(java::JavaParser::new()));

        Self { parsers }
    }

    fn parser_for_path(&self, path: &str) -> Result<&(dyn LanguageParserTrait + Send + Sync)> {
        let extension = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        self.parsers.get(extension)
            .map(|parser| parser.as_ref())
            .ok_or_else(|| anyhow::anyhow!("Unsupported file type: {}", extension))
    }

    pub async fn parse_file(&self, path: &str) -> Result<FileAnalysis> {
        let content = tokio::fs::read_to_string(path).await?;
        self.parse_content(path, &content)
    }

    /// Parse caller-supplied source under a stable repository-relative path.
    /// This is used by incremental IDE/repository learning so CKB can reparse
    /// only verified changed files without requiring a full repository rescan.
    /// The caller remains responsible for source authorization and transport;
    /// the parser never uploads source on its own.
    pub fn parse_content(&self, path: &str, content: &str) -> Result<FileAnalysis> {
        self.parser_for_path(path)?.parse(path, content)
    }

    pub fn is_supported_extension(&self, ext: &std::ffi::OsStr) -> bool {
        ext.to_str().map_or(false, |e| self.parsers.contains_key(e))
    }
}

impl Default for LanguageParser {
    fn default() -> Self {
        Self::new()
    }
}

pub trait LanguageParserTrait: Send + Sync {
    fn parse(&self, path: &str, content: &str) -> Result<FileAnalysis>;
}

/// Persistable parsed source evidence. Storing this normalized AST-derived
/// representation lets incremental learning rebuild cross-file resolution after
/// a small change without reparsing every unchanged source file. It contains no
/// source text.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileAnalysis {
    pub path: String,
    pub nodes: Vec<Node>,
    pub imports: Vec<Import>,
    pub exports: Vec<Symbol>,
    pub calls: Vec<FunctionCall>,
    pub type_relations: Vec<TypeRelation>,
}
