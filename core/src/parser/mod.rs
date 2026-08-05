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
        
        // Register parsers for each language
        parsers.insert("ts".to_string(), Box::new(typescript::TypeScriptParser::new()));
        parsers.insert("js".to_string(), Box::new(typescript::JavaScriptParser::new()));
        parsers.insert("py".to_string(), Box::new(python::PythonParser::new()));
        parsers.insert("go".to_string(), Box::new(go::GoParser::new()));
        parsers.insert("rs".to_string(), Box::new(rust::RustParser::new()));
        parsers.insert("java".to_string(), Box::new(java::JavaParser::new()));
        
        Self { parsers }
    }
    
    pub async fn parse_file(&self, path: &str) -> Result<FileAnalysis> {
        let extension = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        
        let parser = self.parsers.get(extension)
            .ok_or_else(|| anyhow::anyhow!("Unsupported file type: {}", extension))?;
        
        let content = tokio::fs::read_to_string(path).await?;
        parser.parse(path, &content)
    }
    
    pub fn is_supported_extension(&self, ext: &std::ffi::OsStr) -> bool {
        ext.to_str().map_or(false, |e| self.parsers.contains_key(e))
    }
}

pub trait LanguageParserTrait: Send + Sync {
    fn parse(&self, path: &str, content: &str) -> Result<FileAnalysis>;
}

#[derive(Debug, Clone)]
pub struct FileAnalysis {
    pub path: String,
    pub nodes: Vec<Node>,
    pub imports: Vec<Import>,
    pub exports: Vec<Symbol>,
    pub calls: Vec<FunctionCall>,
    pub type_relations: Vec<TypeRelation>,
}

