//! Go parser using tree-sitter

use tree_sitter::{Node, Parser};
use crate::parser::{FileAnalysis, LanguageParserTrait};
use crate::types::{FunctionCall, Import, ImportKind, Node as CkbNode, NodeId, NodeKind, Symbol, SymbolKind};
use anyhow::Result;
use std::collections::HashMap;

pub struct GoParser { parser: std::sync::Mutex<Parser> }

impl GoParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::language()).expect("Failed to load Go grammar");
        Self { parser: std::sync::Mutex::new(parser) }
    }

    fn extract_imports(&self, root: Node, source: &str) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "import_spec" {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if matches!(child.kind(), "interpreted_string_literal" | "raw_string_literal") {
                        let path = child.utf8_text(source.as_bytes()).unwrap_or("").trim_matches('"').trim_matches('`').to_string();
                        if !path.is_empty() { imports.push(Import { source: path, symbols: vec![], kind: ImportKind::Default }); }
                    }
                }
            }
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
        imports
    }

    fn name(&self, node: Node, source: &str) -> Option<String> {
        node.child_by_field_name("name")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(str::to_string)
            .or_else(|| {
                let mut cursor = node.walk();
                node.children(&mut cursor)
                    .find(|n| matches!(n.kind(), "identifier" | "field_identifier" | "type_identifier"))
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                    .map(str::to_string)
            })
    }

    fn exported(name: &str) -> bool { name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) }

    fn extract_exports(&self, root: Node, source: &str) -> Vec<Symbol> {
        let mut exports = Vec::new();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            match child.kind() {
                "function_declaration" => if let Some(name) = self.name(child, source) {
                    let public = Self::exported(&name);
                    exports.push(Symbol { name, kind: SymbolKind::Function, exported: public, public });
                },
                "type_declaration" => {
                    let mut c = child.walk();
                    for spec in child.children(&mut c) {
                        if spec.kind() == "type_spec" {
                            if let Some(name) = self.name(spec, source) {
                                let public = Self::exported(&name);
                                exports.push(Symbol { name, kind: SymbolKind::Type, exported: public, public });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        exports
    }

    fn span_metadata(node: Node) -> HashMap<String, String> {
        let s = node.start_position(); let e = node.end_position();
        let mut m = HashMap::new();
        m.insert("start_line".into(), (s.row + 1).to_string());
        m.insert("start_column".into(), (s.column + 1).to_string());
        m.insert("end_line".into(), (e.row + 1).to_string());
        m.insert("end_column".into(), (e.column + 1).to_string());
        m.insert("byte_start".into(), node.start_byte().to_string());
        m.insert("byte_end".into(), node.end_byte().to_string());
        m.insert("evidence_source".into(), "tree-sitter-ast".into());
        m
    }

    fn push_node(&self, path: &str, node: Node, kind: NodeKind, qualified: String, display: String, nodes: &mut Vec<CkbNode>) {
        let p = node.start_position();
        nodes.push(CkbNode {
            id: NodeId(format!("{}::{}", path, qualified)), kind, name: display, path: path.into(),
            line: (p.row + 1) as u32, column: (p.column + 1) as u32, exports: vec![], imports: vec![], metadata: Self::span_metadata(node),
        });
    }

    fn method_receiver(&self, node: Node, source: &str) -> Option<String> {
        let receiver = node.child_by_field_name("receiver")?;
        let text = receiver.utf8_text(source.as_bytes()).ok()?;
        let mut tokens = text.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|s| !s.is_empty());
        let mut last = None;
        while let Some(t) = tokens.next() { if t != "func" { last = Some(t.to_string()); } }
        last
    }

    fn collect_declarations(&self, path: &str, node: Node, source: &str, nodes: &mut Vec<CkbNode>) {
        match node.kind() {
            "function_declaration" => if let Some(name) = self.name(node, source) { self.push_node(path, node, NodeKind::Function, name.clone(), name, nodes); },
            "method_declaration" => if let Some(name) = self.name(node, source) {
                let receiver = self.method_receiver(node, source).unwrap_or_else(|| "receiver".into());
                self.push_node(path, node, NodeKind::Method, format!("{}.{}", receiver, name), name, nodes);
            },
            "type_spec" => if let Some(name) = self.name(node, source) {
                let kind = node.child_by_field_name("type").map(|n| match n.kind() { "struct_type" => NodeKind::Class, "interface_type" => NodeKind::Interface, _ => NodeKind::Type }).unwrap_or(NodeKind::Type);
                self.push_node(path, node, kind, name.clone(), name, nodes);
            },
            _ => {}
        }
        let mut c = node.walk();
        for child in node.children(&mut c) { self.collect_declarations(path, child, source, nodes); }
    }

    fn call_target(&self, call: Node, source: &str) -> Option<String> {
        let f = call.child_by_field_name("function")?;
        match f.kind() {
            "identifier" => Some(f.utf8_text(source.as_bytes()).ok()?.to_string()),
            "selector_expression" => f.child_by_field_name("field")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .map(str::to_string),
            _ => None,
        }
    }

    fn collect_calls(&self, node: Node, source: &str, callable: Option<&str>, calls: &mut Vec<FunctionCall>) {
        let mut ctx = callable.map(str::to_string);
        if matches!(node.kind(), "function_declaration" | "method_declaration") {
            if let Some(name) = self.name(node, source) {
                ctx = if node.kind() == "method_declaration" {
                    Some(format!("{}.{}", self.method_receiver(node, source).unwrap_or_else(|| "receiver".into()), name))
                } else { Some(name) };
            }
        } else if node.kind() == "call_expression" {
            if let (Some(caller), Some(callee)) = (ctx.as_ref(), self.call_target(node, source)) {
                let p = node.start_position();
                calls.push(FunctionCall { caller_name: caller.clone(), callee_name: callee, line: (p.row + 1) as u32, column: (p.column + 1) as u32 });
            }
        }
        let mut c = node.walk();
        for child in node.children(&mut c) { self.collect_calls(child, source, ctx.as_deref(), calls); }
    }
}

impl LanguageParserTrait for GoParser {
    fn parse(&self, path: &str, content: &str) -> Result<FileAnalysis> {
        let tree = self.parser.lock().unwrap_or_else(|p| p.into_inner()).parse(content, None).ok_or_else(|| anyhow::anyhow!("Failed to parse Go file"))?;
        let root = tree.root_node();
        let imports = self.extract_imports(root, content);
        let exports = self.extract_exports(root, content);
        let mut nodes = Vec::new(); let mut calls = Vec::new();
        let end = root.end_position();
        let mut metadata = HashMap::new();
        metadata.insert("start_line".into(), "1".into()); metadata.insert("start_column".into(), "1".into());
        metadata.insert("end_line".into(), (end.row + 1).to_string()); metadata.insert("end_column".into(), (end.column + 1).to_string());
        metadata.insert("byte_start".into(), "0".into()); metadata.insert("byte_end".into(), content.len().to_string()); metadata.insert("evidence_source".into(), "tree-sitter-ast".into());
        nodes.push(CkbNode { id: NodeId(format!("{}::file", path)), kind: NodeKind::File, name: path.to_string(), path: path.into(), line: 1, column: 1, exports: exports.clone(), imports: imports.clone(), metadata });
        self.collect_declarations(path, root, content, &mut nodes);
        self.collect_calls(root, content, None, &mut calls);
        Ok(FileAnalysis { path: path.to_string(), nodes, imports, exports, calls, type_relations: Vec::new() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_function_starts_with_capital() {
        let p = GoParser::new(); let a = p.parse("hello.go", "package main\nfunc Hello() {}").unwrap();
        let hello = a.exports.iter().find(|s| s.name == "Hello").unwrap(); assert!(hello.public);
        assert!(a.nodes.iter().any(|n| n.id.0 == "hello.go::Hello"));
    }

    #[test]
    fn extracts_method_and_calls() {
        let p = GoParser::new();
        let a = p.parse("svc.go", "package x\ntype Service struct{}\nfunc (s *Service) Run(){ helper() }\nfunc helper(){}").unwrap();
        assert!(a.nodes.iter().any(|n| n.kind == NodeKind::Method && n.name == "Run"));
        assert!(a.calls.iter().any(|c| c.callee_name == "helper"));
    }

    #[test]
    fn handles_empty_and_malformed() {
        let p = GoParser::new(); assert!(p.parse("empty.go", "").is_ok()); assert!(p.parse("bad.go", "package main\nfunc oops( {").is_ok());
    }
}
