//! Python parser using tree-sitter

use tree_sitter::{Parser, Node};
use crate::parser::{LanguageParserTrait, FileAnalysis};
use crate::types::{Node as CkbNode, NodeKind, Symbol, SymbolKind, Import, ImportKind};
use anyhow::Result;
use std::collections::HashSet;

pub struct PythonParser {
    parser: std::sync::Mutex<Parser>,
}

impl PythonParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_python::language())
            .expect("Failed to load Python grammar");
        Self { parser: std::sync::Mutex::new(parser) }
    }
    
    fn extract_imports(&self, node: Node, source: &str) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut cursor = node.walk();
        
        for child in node.children(&mut cursor) {
            match child.kind() {
                "import_statement" => {
                    if let Some(import) = self.parse_import_statement(child, source) {
                        imports.push(import);
                    }
                }
                "import_from_statement" => {
                    if let Some(import) = self.parse_from_import(child, source) {
                        imports.push(import);
                    }
                }
                _ => {}
            }
        }
        
        imports
    }
    
    fn parse_import_statement(&self, node: Node, source: &str) -> Option<Import> {
        let mut symbols = Vec::new();
        let mut cursor = node.walk();
        
        for child in node.children(&mut cursor) {
            if child.kind() == "dotted_name" {
                let name = child.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                symbols.push(name);
            }
        }
        
        if !symbols.is_empty() {
            Some(Import {
                source: symbols[0].clone(),
                symbols: symbols.clone(),
                kind: ImportKind::Named,
            })
        } else {
            None
        }
    }
    
    fn parse_from_import(&self, node: Node, source: &str) -> Option<Import> {
        let mut module_name = None;
        let mut symbols = Vec::new();
        let mut cursor = node.walk();
        
        for child in node.children(&mut cursor) {
            match child.kind() {
                "dotted_name" => {
                    module_name = Some(child.utf8_text(source.as_bytes()).unwrap_or("").to_string());
                }
                "import_list" => {
                    symbols = self.extract_import_list(child, source);
                }
                _ => {}
            }
        }
        
        module_name.map(|module| Import {
            source: module,
            symbols,
            kind: ImportKind::Named,
        })
    }
    
    fn extract_import_list(&self, node: Node, source: &str) -> Vec<String> {
        let mut symbols = Vec::new();
        let mut cursor = node.walk();
        
        for child in node.children(&mut cursor) {
            if child.kind() == "dotted_name" {
                symbols.push(child.utf8_text(source.as_bytes()).unwrap_or("").to_string());
            }
        }
        
        symbols
    }
    
    fn extract_exports(&self, node: Node, source: &str) -> Vec<Symbol> {
        let mut exports = Vec::new();
        let mut cursor = node.walk();
        
        for child in node.children(&mut cursor) {
            match child.kind() {
                "class_definition" => {
                    if let Some(name) = self.get_class_name(child, source) {
                        exports.push(Symbol {
                            name,
                            kind: SymbolKind::Class,
                            exported: true,
                            public: true,
                        });
                    }
                }
                "function_definition" => {
                    if let Some(name) = self.get_function_name(child, source) {
                        exports.push(Symbol {
                            name,
                            kind: SymbolKind::Function,
                            exported: true,
                            public: true,
                        });
                    }
                }
                "assignment" => {
                    if let Some(name) = self.get_assignment_name(child, source) {
                        exports.push(Symbol {
                            name,
                            kind: SymbolKind::Variable,
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
    
    fn get_class_name(&self, node: Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return Some(child.utf8_text(source.as_bytes()).unwrap_or("").to_string());
            }
        }
        None
    }
    
    fn get_function_name(&self, node: Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return Some(child.utf8_text(source.as_bytes()).unwrap_or("").to_string());
            }
        }
        None
    }
    
    fn get_assignment_name(&self, node: Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return Some(child.utf8_text(source.as_bytes()).unwrap_or("").to_string());
            }
        }
        None
    }
}

impl LanguageParserTrait for PythonParser {
    fn parse(&self, path: &str, content: &str) -> Result<FileAnalysis> {
        let tree = self.parser.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse Python file"))?;
        
        let root = tree.root_node();
        
        let imports = self.extract_imports(root, content);
        let exports = self.extract_exports(root, content);
        
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SymbolKind;

    #[test]
    fn extracts_a_top_level_function() {
        let parser = PythonParser::new();
        let analysis = parser.parse("hello.py", "def hello():\n    pass\n").expect("parse should succeed");

        assert!(analysis.exports.iter().any(|s| s.name == "hello" && s.kind == SymbolKind::Function));
    }

    #[test]
    fn extracts_a_class() {
        let parser = PythonParser::new();
        let analysis = parser.parse("widget.py", "class Widget:\n    def __init__(self):\n        pass\n").expect("parse should succeed");

        assert!(analysis.exports.iter().any(|s| s.name == "Widget" && s.kind == SymbolKind::Class));
    }

    #[test]
    fn extracts_imports() {
        let parser = PythonParser::new();
        let analysis = parser.parse("main.py", "import os\nfrom collections import OrderedDict\n").expect("parse should succeed");

        assert!(analysis.imports.iter().any(|i| i.source == "os"));
        assert!(analysis.imports.iter().any(|i| i.source == "collections"));
    }

    #[test]
    fn handles_empty_content_without_panicking() {
        let parser = PythonParser::new();
        assert!(parser.parse("empty.py", "").is_ok());
    }

    #[test]
    fn handles_malformed_syntax_without_panicking() {
        let parser = PythonParser::new();
        let result = parser.parse("broken.py", "def oops(:\n    this is not : valid python @@@");
        assert!(result.is_ok());
    }

    #[test]
    fn survives_repeated_parses_on_the_same_instance() {
        let parser = PythonParser::new();
        for i in 0..50 {
            let src = format!("def f{}():\n    pass\n", i);
            let analysis = parser.parse("repeat.py", &src).expect("parse should succeed");
            assert!(analysis.exports.iter().any(|s| s.name == format!("f{}", i)));
        }
    }
}
