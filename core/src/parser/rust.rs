//! Rust parser using tree-sitter

use tree_sitter::{Node, Parser};
use crate::parser::{FileAnalysis, LanguageParserTrait};
use crate::types::{
    FunctionCall, Import, ImportKind, Node as CkbNode, NodeId, NodeKind,
    Symbol, SymbolKind, TypeRelation, TypeRelationKind,
};
use anyhow::Result;
use std::collections::HashMap;

pub struct RustParser {
    parser: std::sync::Mutex<Parser>,
}

impl RustParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_rust::language()).expect("Failed to load Rust grammar");
        Self { parser: std::sync::Mutex::new(parser) }
    }

    fn extract_imports(&self, node: Node, source: &str) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "use_declaration" {
                if let Some(import) = self.parse_use_declaration(child, source) { imports.push(import); }
            }
        }
        imports
    }

    fn parse_use_declaration(&self, node: Node, source: &str) -> Option<Import> {
        let text = node.utf8_text(source.as_bytes()).ok()?.trim();
        let body = text.strip_prefix("use")?.trim().trim_end_matches(';').trim();
        let source_path = body
            .trim_start_matches("::")
            .split("::")
            .next()
            .unwrap_or("")
            .trim_matches(|c: char| c == '{' || c == ' ')
            .to_string();
        if source_path.is_empty() { return None; }
        let symbols = body
            .replace('{', "")
            .replace('}', "")
            .split("::")
            .map(|s| s.trim().trim_end_matches(',').to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Some(Import { source: source_path, symbols, kind: ImportKind::Named })
    }

    fn item_name(&self, node: Node, source: &str) -> Option<String> {
        if let Some(name) = node.child_by_field_name("name") {
            let text = name.utf8_text(source.as_bytes()).unwrap_or("");
            if !text.is_empty() { return Some(text.to_string()); }
        }
        let mut cursor = node.walk();
        let result = node.children(&mut cursor)
            .find(|n| matches!(n.kind(), "identifier" | "type_identifier"))
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(str::to_string);
        result
    }

    fn is_public(&self, node: Node) -> bool {
        let mut cursor = node.walk();
        let result = node.children(&mut cursor).any(|n| n.kind() == "visibility_modifier");
        result
    }

    fn extract_exports(&self, root: Node, source: &str) -> Vec<Symbol> {
        let mut exports = Vec::new();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            let kind = match child.kind() {
                "function_item" => Some(SymbolKind::Function),
                "struct_item" | "enum_item" => Some(SymbolKind::Class),
                "trait_item" => Some(SymbolKind::Interface),
                "const_item" | "static_item" => Some(SymbolKind::Constant),
                "type_item" => Some(SymbolKind::Type),
                _ => None,
            };
            if let (Some(kind), Some(name)) = (kind, self.item_name(child, source)) {
                let public = self.is_public(child);
                exports.push(Symbol { name, kind, exported: public, public });
            }
        }
        exports
    }

    fn span_metadata(node: Node) -> HashMap<String, String> {
        let start = node.start_position();
        let end = node.end_position();
        let mut metadata = HashMap::new();
        metadata.insert("start_line".into(), (start.row + 1).to_string());
        metadata.insert("start_column".into(), (start.column + 1).to_string());
        metadata.insert("end_line".into(), (end.row + 1).to_string());
        metadata.insert("end_column".into(), (end.column + 1).to_string());
        metadata.insert("byte_start".into(), node.start_byte().to_string());
        metadata.insert("byte_end".into(), node.end_byte().to_string());
        metadata.insert("evidence_source".into(), "tree-sitter-ast".into());
        metadata
    }

    fn push_node(&self, path: &str, node: Node, kind: NodeKind, qualified: String, name: String, nodes: &mut Vec<CkbNode>) {
        let pos = node.start_position();
        nodes.push(CkbNode {
            id: NodeId(format!("{}::{}", path, qualified)),
            kind,
            name,
            path: path.into(),
            line: (pos.row + 1) as u32,
            column: (pos.column + 1) as u32,
            exports: Vec::new(),
            imports: Vec::new(),
            metadata: Self::span_metadata(node),
        });
    }

    fn impl_target(&self, node: Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        let candidates: Vec<Node> = node.children(&mut cursor)
            .filter(|n| matches!(n.kind(), "type_identifier" | "scoped_type_identifier" | "generic_type"))
            .collect();
        candidates.last()
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(str::to_string)
    }

    fn collect_declarations(
        &self,
        path: &str,
        node: Node,
        source: &str,
        impl_ctx: Option<&str>,
        nodes: &mut Vec<CkbNode>,
        relations: &mut Vec<TypeRelation>,
    ) {
        let mut next_impl = impl_ctx.map(str::to_string);
        match node.kind() {
            "function_item" => {
                if let Some(name) = self.item_name(node, source) {
                    if let Some(target) = impl_ctx {
                        self.push_node(path, node, NodeKind::Method, format!("{}.{}", target, name), name, nodes);
                    } else {
                        self.push_node(path, node, NodeKind::Function, name.clone(), name, nodes);
                    }
                }
            }
            "struct_item" => if let Some(name) = self.item_name(node, source) { self.push_node(path, node, NodeKind::Class, name.clone(), name, nodes); },
            "enum_item" => if let Some(name) = self.item_name(node, source) { self.push_node(path, node, NodeKind::Enum, name.clone(), name, nodes); },
            "trait_item" => if let Some(name) = self.item_name(node, source) { self.push_node(path, node, NodeKind::Interface, name.clone(), name, nodes); },
            "type_item" => if let Some(name) = self.item_name(node, source) { self.push_node(path, node, NodeKind::Type, name.clone(), name, nodes); },
            "impl_item" => {
                next_impl = self.impl_target(node, source);
                let mut cursor = node.walk();
                let ids: Vec<String> = node.children(&mut cursor)
                    .filter(|n| matches!(n.kind(), "type_identifier" | "scoped_type_identifier"))
                    .filter_map(|n| n.utf8_text(source.as_bytes()).ok().map(str::to_string))
                    .collect();
                if ids.len() >= 2 {
                    relations.push(TypeRelation {
                        source_type: ids.last().cloned().unwrap_or_default(),
                        target_type: ids.first().cloned().unwrap_or_default(),
                        kind: TypeRelationKind::Implements,
                    });
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_declarations(path, child, source, next_impl.as_deref(), nodes, relations);
        }
    }

    fn call_target(&self, call: Node, source: &str, impl_ctx: Option<&str>) -> Option<String> {
        let function = call.child_by_field_name("function")?;
        match function.kind() {
            "identifier" | "scoped_identifier" => {
                let text = function.utf8_text(source.as_bytes()).ok()?;
                Some(text.rsplit("::").next().unwrap_or(text).to_string())
            }
            "field_expression" => {
                let value = function.child_by_field_name("value")
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                    .unwrap_or("");
                let field = function.child_by_field_name("field")
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())?;
                if value == "self" {
                    impl_ctx.map(|t| format!("{}.{}", t, field)).or_else(|| Some(field.to_string()))
                } else {
                    Some(field.to_string())
                }
            }
            _ => None,
        }
    }

    fn collect_calls(
        &self,
        node: Node,
        source: &str,
        impl_ctx: Option<&str>,
        callable_ctx: Option<&str>,
        calls: &mut Vec<FunctionCall>,
    ) {
        let mut next_impl = impl_ctx.map(str::to_string);
        let mut next_callable = callable_ctx.map(str::to_string);
        if node.kind() == "impl_item" {
            next_impl = self.impl_target(node, source);
        } else if node.kind() == "function_item" {
            if let Some(name) = self.item_name(node, source) {
                next_callable = Some(next_impl.as_ref().map(|t| format!("{}.{}", t, name)).unwrap_or(name));
            }
        } else if node.kind() == "call_expression" {
            if let (Some(caller), Some(callee)) = (next_callable.as_ref(), self.call_target(node, source, next_impl.as_deref())) {
                let pos = node.start_position();
                calls.push(FunctionCall {
                    caller_name: caller.clone(),
                    callee_name: callee,
                    line: (pos.row + 1) as u32,
                    column: (pos.column + 1) as u32,
                });
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_calls(child, source, next_impl.as_deref(), next_callable.as_deref(), calls);
        }
    }
}

impl LanguageParserTrait for RustParser {
    fn parse(&self, path: &str, content: &str) -> Result<FileAnalysis> {
        let tree = self.parser.lock().unwrap_or_else(|p| p.into_inner())
            .parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse Rust file"))?;
        let root = tree.root_node();
        let imports = self.extract_imports(root, content);
        let exports = self.extract_exports(root, content);
        let mut nodes = Vec::new();
        let mut calls = Vec::new();
        let mut type_relations = Vec::new();

        let end = root.end_position();
        let mut file_meta = HashMap::new();
        file_meta.insert("start_line".into(), "1".into());
        file_meta.insert("start_column".into(), "1".into());
        file_meta.insert("end_line".into(), (end.row + 1).to_string());
        file_meta.insert("end_column".into(), (end.column + 1).to_string());
        file_meta.insert("byte_start".into(), "0".into());
        file_meta.insert("byte_end".into(), content.len().to_string());
        file_meta.insert("evidence_source".into(), "tree-sitter-ast".into());
        nodes.push(CkbNode {
            id: NodeId(format!("{}::file", path)), kind: NodeKind::File, name: path.to_string(), path: path.into(),
            line: 1, column: 1, exports: exports.clone(), imports: imports.clone(), metadata: file_meta,
        });

        self.collect_declarations(path, root, content, None, &mut nodes, &mut type_relations);
        self.collect_calls(root, content, None, None, &mut calls);
        Ok(FileAnalysis { path: path.to_string(), nodes, imports, exports, calls, type_relations })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_function_node_and_span() {
        let parser = RustParser::new();
        let analysis = parser.parse("hello.rs", "fn hello() {}").unwrap();
        assert!(analysis.nodes.iter().any(|n| n.id.0 == "hello.rs::hello" && n.kind == NodeKind::Function));
        let n = analysis.nodes.iter().find(|n| n.id.0 == "hello.rs::hello").unwrap();
        assert!(n.metadata.get("end_line").is_some());
    }

    #[test]
    fn extracts_impl_methods_and_self_calls() {
        let parser = RustParser::new();
        let src = "struct User; impl User { fn save(&self) { self.validate(); } fn validate(&self) {} }";
        let analysis = parser.parse("user.rs", src).unwrap();
        assert!(analysis.nodes.iter().any(|n| n.id.0 == "user.rs::User.save"));
        assert!(analysis.calls.iter().any(|c| c.caller_name == "User.save" && c.callee_name == "User.validate"));
    }

    #[test]
    fn extracts_a_public_struct() {
        let parser = RustParser::new();
        let analysis = parser.parse("lib.rs", "pub struct Widget { id: u32 }").unwrap();
        assert_eq!(analysis.exports[0].name, "Widget");
        assert!(analysis.exports[0].public);
    }

    #[test]
    fn extracts_use_declarations_as_imports() {
        let parser = RustParser::new();
        let analysis = parser.parse("main.rs", "use std::collections::HashMap;\nfn main() {}").unwrap();
        assert_eq!(analysis.imports[0].source, "std");
    }

    #[test]
    fn handles_empty_and_malformed_content() {
        let parser = RustParser::new();
        assert!(parser.parse("empty.rs", "").is_ok());
        assert!(parser.parse("broken.rs", "fn oops( { this is not valid rust @@@ %%").is_ok());
    }

    #[test]
    fn survives_repeated_parses_on_the_same_instance() {
        let parser = RustParser::new();
        for i in 0..50 {
            let src = format!("fn f{}() {{}}", i);
            let analysis = parser.parse("repeat.rs", &src).unwrap();
            assert_eq!(analysis.exports[0].name, format!("f{}", i));
        }
    }
}
