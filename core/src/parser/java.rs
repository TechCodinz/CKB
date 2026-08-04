//! Java parser using tree-sitter

use tree_sitter::{Parser, Node};
use crate::parser::{LanguageParserTrait, FileAnalysis};
use crate::types::{Node as CkbNode, NodeKind, Symbol, SymbolKind, Import, ImportKind};
use anyhow::Result;

pub struct JavaParser {
    parser: std::sync::Mutex<Parser>,
}

impl JavaParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_java::language())
            .expect("Failed to load Java grammar");
        Self { parser: std::sync::Mutex::new(parser) }
    }

    fn extract_imports(&self, node: Node, source: &str) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == "import_declaration" {
                if let Some(import) = self.parse_import(child, source) {
                    imports.push(import);
                }
            }
        }

        imports
    }

    fn parse_import(&self, node: Node, source: &str) -> Option<Import> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "scoped_identifier" {
                let text = child.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                let parts: Vec<String> = text.split('.').map(|s| s.to_string()).collect();
                let last = parts.last().cloned().unwrap_or_default();
                return Some(Import {
                    source: text,
                    symbols: vec![last],
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
                "class_declaration" => {
                    let is_pub = self.has_modifier(child, source, "public");
                    if let Some(name) = self.get_item_name(child, source) {
                        exports.push(Symbol {
                            name,
                            kind: SymbolKind::Class,
                            exported: is_pub,
                            public: is_pub,
                        });
                    }
                    // Extract methods from class body
                    exports.extend(self.extract_class_methods(child, source));
                }
                "interface_declaration" => {
                    let is_pub = self.has_modifier(child, source, "public");
                    if let Some(name) = self.get_item_name(child, source) {
                        exports.push(Symbol {
                            name,
                            kind: SymbolKind::Interface,
                            exported: is_pub,
                            public: is_pub,
                        });
                    }
                }
                "enum_declaration" => {
                    let is_pub = self.has_modifier(child, source, "public");
                    if let Some(name) = self.get_item_name(child, source) {
                        exports.push(Symbol {
                            name,
                            kind: SymbolKind::Class,
                            exported: is_pub,
                            public: is_pub,
                        });
                    }
                }
                _ => {}
            }
        }

        exports
    }

    fn has_modifier(&self, node: Node, source: &str, modifier: &str) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" {
                let text = child.utf8_text(source.as_bytes()).unwrap_or("");
                if text.contains(modifier) {
                    return true;
                }
            }
        }
        false
    }

    fn get_item_name(&self, node: Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return Some(child.utf8_text(source.as_bytes()).unwrap_or("").to_string());
            }
        }
        None
    }

    fn extract_class_methods(&self, node: Node, source: &str) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == "class_body" {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    if inner.kind() == "method_declaration" {
                        let is_pub = self.has_modifier(inner, source, "public");
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

impl LanguageParserTrait for JavaParser {
    fn parse(&self, path: &str, content: &str) -> Result<FileAnalysis> {
        let tree = self.parser.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse Java file"))?;

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
    fn extracts_a_public_class() {
        let parser = JavaParser::new();
        let analysis = parser.parse("Widget.java", "public class Widget {\n    public void run() {}\n}\n")
            .expect("parse should succeed");

        let widget = analysis.exports.iter().find(|s| s.name == "Widget");
        assert!(widget.is_some());
        let widget = widget.unwrap();
        assert_eq!(widget.kind, SymbolKind::Class);
        assert!(widget.public);
        assert!(widget.exported);
    }

    #[test]
    fn marks_package_private_class_as_not_public() {
        let parser = JavaParser::new();
        let analysis = parser.parse("Internal.java", "class Internal {}\n").expect("parse should succeed");

        let internal = analysis.exports.iter().find(|s| s.name == "Internal").expect("class should be extracted");
        assert!(!internal.public);
    }

    #[test]
    fn extracts_an_interface() {
        let parser = JavaParser::new();
        let analysis = parser.parse("Shape.java", "public interface Shape {\n    double area();\n}\n")
            .expect("parse should succeed");

        assert!(analysis.exports.iter().any(|s| s.name == "Shape" && s.kind == SymbolKind::Interface));
    }

    #[test]
    fn handles_empty_content_without_panicking() {
        let parser = JavaParser::new();
        assert!(parser.parse("Empty.java", "").is_ok());
    }

    #[test]
    fn handles_malformed_syntax_without_panicking() {
        let parser = JavaParser::new();
        let result = parser.parse("Broken.java", "public class {{{ this is not valid java @@@");
        assert!(result.is_ok());
    }
}
