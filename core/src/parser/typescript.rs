//! TypeScript/JavaScript parser using tree-sitter

use tree_sitter::{Parser, Node};
use crate::parser::{LanguageParserTrait, FileAnalysis};
use crate::types::{Node as CkbNode, NodeKind, Symbol, SymbolKind, Import, ImportKind};
use anyhow::Result;
use std::collections::HashSet;

pub struct TypeScriptParser {
    parser: std::sync::Mutex<Parser>,
}

impl TypeScriptParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_typescript::language_tsx())
            .expect("Failed to load TypeScript grammar");
        Self { parser: std::sync::Mutex::new(parser) }
    }
    
    fn extract_imports(&self, node: Node, source: &str) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut cursor = node.walk();
        
        for child in node.children(&mut cursor) {
            if child.kind() == "import_statement" {
                if let Some(import) = self.parse_import_statement(child, source) {
                    imports.push(import);
                }
            }
        }
        
        imports
    }
    
    fn parse_import_statement(&self, node: Node, source: &str) -> Option<Import> {
        let mut source_str = None;
        let mut symbols = Vec::new();
        let mut cursor = node.walk();
        
        for child in node.children(&mut cursor) {
            match child.kind() {
                "string" => {
                    source_str = Some(child.utf8_text(source.as_bytes()).unwrap().to_string());
                }
                "import_clause" => {
                    symbols = self.extract_import_symbols(child, source);
                }
                _ => {}
            }
        }
        
        source_str.map(|src| Import {
            source: src.trim_matches('"').trim_matches('\'').to_string(),
            symbols,
            kind: ImportKind::Named, // Simplified for now
        })
    }
    
    fn extract_import_symbols(&self, node: Node, source: &str) -> Vec<String> {
        let mut symbols = Vec::new();
        let mut cursor = node.walk();
        
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                symbols.push(child.utf8_text(source.as_bytes()).unwrap().to_string());
            }
        }
        
        symbols
    }
    
    fn extract_exports(&self, node: Node, source: &str) -> Vec<Symbol> {
        let mut exports = Vec::new();
        let mut cursor = node.walk();
        
        for child in node.children(&mut cursor) {
            match child.kind() {
                "export_statement" => {
                    if let Some(symbol) = self.parse_export_statement(child, source) {
                        exports.push(symbol);
                    }
                }
                "class_declaration" => {
                    if let Some(name) = self.get_node_name(child, source) {
                        exports.push(Symbol {
                            name,
                            kind: SymbolKind::Class,
                            exported: true,
                            public: true,
                        });
                    }
                }
                "function_declaration" => {
                    if let Some(name) = self.get_node_name(child, source) {
                        exports.push(Symbol {
                            name,
                            kind: SymbolKind::Function,
                            exported: true,
                            public: true,
                        });
                    }
                }
                _ => {}
            }
        }
        
        exports
    }
    
    fn parse_export_statement(&self, node: Node, source: &str) -> Option<Symbol> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "class_declaration" || child.kind() == "function_declaration" {
                if let Some(name) = self.get_node_name(child, source) {
                    return Some(Symbol {
                        name,
                        kind: if child.kind() == "class_declaration" {
                            SymbolKind::Class
                        } else {
                            SymbolKind::Function
                        },
                        exported: true,
                        public: true,
                    });
                }
            }
        }
        None
    }
    
    fn get_node_name(&self, node: Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return Some(child.utf8_text(source.as_bytes()).unwrap().to_string());
            }
        }
        None
    }
}

impl LanguageParserTrait for TypeScriptParser {
    fn parse(&self, path: &str, content: &str) -> Result<FileAnalysis> {
        let tree = self.parser.lock().unwrap().parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse file"))?;
        
        let root = tree.root_node();
        
        let imports = self.extract_imports(root, content);
        let exports = self.extract_exports(root, content);
        
        // Create nodes for each significant declaration
        let mut nodes = Vec::new();
        
        // Add file node
        nodes.push(CkbNode {
            id: crate::types::NodeId(format!("{}::file", path)),
            kind: NodeKind::File,
            name: path.to_string(),
            path: path.into(),
            line: 0,
            column: 0,
            exports: exports.clone(),
            imports: imports.clone(),
            metadata: Default::default(),
        });
        
        Ok(FileAnalysis {
            path: path.to_string(),
            nodes,
            imports,
            exports,
            calls: Vec::new(),
            type_relations: Vec::new(),
        })
    }
}

// JavaScript parser (reuses TypeScript parser with JS grammar)
pub struct JavaScriptParser {
    parser: std::sync::Mutex<Parser>,
}

impl JavaScriptParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_typescript::language_tsx())
            .expect("Failed to load JavaScript grammar");
        Self { parser: std::sync::Mutex::new(parser) }
    }
}

impl LanguageParserTrait for JavaScriptParser {
    fn parse(&self, path: &str, content: &str) -> Result<FileAnalysis> {
        let tree = self.parser.lock().unwrap().parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse JavaScript file"))?;

        let root = tree.root_node();

        // Reuse same extraction logic as TypeScript
        let ts_parser = TypeScriptParser::new();
        let imports = ts_parser.extract_imports(root, content);
        let exports = ts_parser.extract_exports(root, content);

        let mut nodes = Vec::new();

        nodes.push(CkbNode {
            id: crate::types::NodeId(format!("{}::file", path)),
            kind: NodeKind::File,
            name: path.to_string(),
            path: path.into(),
            line: 0,
            column: 0,
            exports: exports.clone(),
            imports: imports.clone(),
            metadata: Default::default(),
        });

        Ok(FileAnalysis {
            path: path.to_string(),
            nodes,
            imports,
            exports,
            calls: Vec::new(),
            type_relations: Vec::new(),
        })
    }
}
