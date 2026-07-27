//! Go parser using tree-sitter

use tree_sitter::{Parser, Node};
use crate::parser::{LanguageParserTrait, FileAnalysis};
use crate::types::{Node as CkbNode, NodeKind, Symbol, SymbolKind, Import, ImportKind};
use anyhow::Result;

pub struct GoParser {
    parser: std::sync::Mutex<Parser>,
}

impl GoParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language())
            .expect("Failed to load Go grammar");
        Self { parser: std::sync::Mutex::new(parser) }
    }
    
    fn extract_imports(&self, node: Node, source: &str) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut cursor = node.walk();
        
        for child in node.children(&mut cursor) {
            if child.kind() == "import_declaration" {
                if let Some(import) = self.parse_import_declaration(child, source) {
                    imports.push(import);
                }
            }
        }
        
        imports
    }
    
    fn parse_import_declaration(&self, node: Node, source: &str) -> Option<Import> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "import_spec" {
                return self.parse_import_spec(child, source);
            }
        }
        None
    }
    
    fn parse_import_spec(&self, node: Node, source: &str) -> Option<Import> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "interpreted_string_literal" {
                let path = child.utf8_text(source.as_bytes()).unwrap()
                    .trim_matches('"')
                    .to_string();
                
                return Some(Import {
                    source: path,
                    symbols: vec![],
                    kind: ImportKind::Default,
                });
            }
        }
        None
    }
    
    fn extract_exports(&self, node: Node, source: &str) -> Vec<Symbol> {
        let mut exports = Vec::new();
        let mut cursor = node.walk();
        
        for child in node.children(&mut cursor) {
            match child.kind() {
                "function_declaration" => {
                    if let Some(name) = self.get_function_name(child, source) {
                        // In Go, exported functions start with capital letter
                        let exported = name.chars().next().map_or(false, |c| c.is_uppercase());
                        exports.push(Symbol {
                            name,
                            kind: SymbolKind::Function,
                            exported,
                            public: exported,
                        });
                    }
                }
                "type_declaration" => {
                    exports.extend(self.extract_type_declaration(child, source));
                }
                "var_declaration" => {
                    exports.extend(self.extract_var_declaration(child, source));
                }
                _ => {}
            }
        }
        
        exports
    }
    
    fn get_function_name(&self, node: Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return Some(child.utf8_text(source.as_bytes()).unwrap().to_string());
            }
        }
        None
    }
    
    fn extract_type_declaration(&self, node: Node, source: &str) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let mut cursor = node.walk();
        
        for child in node.children(&mut cursor) {
            if child.kind() == "type_spec" {
                if let Some(name) = self.get_type_name(child, source) {
                    let exported = name.chars().next().map_or(false, |c| c.is_uppercase());
                    symbols.push(Symbol {
                        name,
                        kind: SymbolKind::Type,
                        exported,
                        public: exported,
                    });
                }
            }
        }
        
        symbols
    }
    
    fn get_type_name(&self, node: Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_identifier" {
                return Some(child.utf8_text(source.as_bytes()).unwrap().to_string());
            }
        }
        None
    }
    
    fn extract_var_declaration(&self, node: Node, source: &str) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let mut cursor = node.walk();
        
        for child in node.children(&mut cursor) {
            if child.kind() == "var_spec" {
                if let Some(name) = self.get_var_name(child, source) {
                    let exported = name.chars().next().map_or(false, |c| c.is_uppercase());
                    symbols.push(Symbol {
                        name,
                        kind: SymbolKind::Variable,
                        exported,
                        public: exported,
                    });
                }
            }
        }
        
        symbols
    }
    
    fn get_var_name(&self, node: Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return Some(child.utf8_text(source.as_bytes()).unwrap().to_string());
            }
        }
        None
    }
}

impl LanguageParserTrait for GoParser {
    fn parse(&self, path: &str, content: &str) -> Result<FileAnalysis> {
        let tree = self.parser.lock().unwrap().parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse Go file"))?;
        
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
