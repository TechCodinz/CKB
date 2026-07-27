//! Rust parser using tree-sitter

use tree_sitter::{Parser, Node};
use crate::parser::{LanguageParserTrait, FileAnalysis};
use crate::types::{Node as CkbNode, NodeKind, Symbol, SymbolKind, Import, ImportKind};
use anyhow::Result;

pub struct RustParser {
    parser: Parser,
}

impl RustParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("Failed to load Rust grammar");
        Self { parser }
    }

    fn extract_imports(&self, node: Node, source: &str) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == "use_declaration" {
                if let Some(import) = self.parse_use_declaration(child, source) {
                    imports.push(import);
                }
            }
        }

        imports
    }

    fn parse_use_declaration(&self, node: Node, source: &str) -> Option<Import> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "scoped_identifier" || child.kind() == "use_wildcard"
                || child.kind() == "use_list" || child.kind() == "identifier"
                || child.kind() == "scoped_use_list"
            {
                let text = child.utf8_text(source.as_bytes()).unwrap().to_string();
                let parts: Vec<String> = text.split("::").map(|s| s.trim().to_string()).collect();
                let source_path = parts.first().cloned().unwrap_or_default();
                return Some(Import {
                    source: source_path,
                    symbols: parts,
                    kind: ImportKind::Named,
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
                "function_item" => {
                    let is_pub = self.is_public(child, source);
                    if let Some(name) = self.get_item_name(child, source) {
                        exports.push(Symbol {
                            name,
                            kind: SymbolKind::Function,
                            exported: is_pub,
                            public: is_pub,
                        });
                    }
                }
                "struct_item" | "enum_item" => {
                    let is_pub = self.is_public(child, source);
                    if let Some(name) = self.get_item_name(child, source) {
                        exports.push(Symbol {
                            name,
                            kind: SymbolKind::Class,
                            exported: is_pub,
                            public: is_pub,
                        });
                    }
                }
                "trait_item" => {
                    let is_pub = self.is_public(child, source);
                    if let Some(name) = self.get_item_name(child, source) {
                        exports.push(Symbol {
                            name,
                            kind: SymbolKind::Interface,
                            exported: is_pub,
                            public: is_pub,
                        });
                    }
                }
                "const_item" | "static_item" => {
                    let is_pub = self.is_public(child, source);
                    if let Some(name) = self.get_item_name(child, source) {
                        exports.push(Symbol {
                            name,
                            kind: SymbolKind::Constant,
                            exported: is_pub,
                            public: is_pub,
                        });
                    }
                }
                "type_item" => {
                    let is_pub = self.is_public(child, source);
                    if let Some(name) = self.get_item_name(child, source) {
                        exports.push(Symbol {
                            name,
                            kind: SymbolKind::Type,
                            exported: is_pub,
                            public: is_pub,
                        });
                    }
                }
                "impl_item" => {
                    // Extract methods from impl blocks
                    exports.extend(self.extract_impl_methods(child, source));
                }
                _ => {}
            }
        }

        exports
    }

    fn is_public(&self, node: Node, source: &str) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "visibility_modifier" {
                return true;
            }
        }
        false
    }

    fn get_item_name(&self, node: Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" || child.kind() == "type_identifier" {
                return Some(child.utf8_text(source.as_bytes()).unwrap().to_string());
            }
        }
        None
    }

    fn extract_impl_methods(&self, node: Node, source: &str) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == "declaration_list" {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    if inner.kind() == "function_item" {
                        let is_pub = self.is_public(inner, source);
                        if let Some(name) = self.get_item_name(inner, source) {
                            symbols.push(Symbol {
                                name,
                                kind: SymbolKind::Function,
                                exported: is_pub,
                                public: is_pub,
                            });
                        }
                    }
                }
            }
        }

        symbols
    }
}

impl LanguageParserTrait for RustParser {
    fn parse(&self, path: &str, content: &str) -> Result<FileAnalysis> {
        let tree = self.parser.parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse Rust file"))?;

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
