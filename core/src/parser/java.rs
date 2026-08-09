//! Java parser using tree-sitter

use tree_sitter::{Node, Parser};
use crate::parser::{FileAnalysis, LanguageParserTrait};
use crate::types::{
    FunctionCall, Import, ImportKind, Node as CkbNode, NodeId, NodeKind,
    Symbol, SymbolKind, TypeRelation, TypeRelationKind,
};
use anyhow::Result;
use std::collections::HashMap;

pub struct JavaParser { parser: std::sync::Mutex<Parser> }

impl JavaParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_java::language()).expect("Failed to load Java grammar");
        Self { parser: std::sync::Mutex::new(parser) }
    }

    fn extract_imports(&self, root: Node, source: &str) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if child.kind() == "import_declaration" {
                let text = child.utf8_text(source.as_bytes()).unwrap_or("")
                    .trim().trim_start_matches("import").trim().trim_start_matches("static").trim().trim_end_matches(';').trim().to_string();
                if !text.is_empty() {
                    let last = text.rsplit('.').next().unwrap_or(&text).to_string();
                    imports.push(Import { source: text, symbols: vec![last], kind: ImportKind::Named });
                }
            }
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
                    .find(|n| n.kind() == "identifier")
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                    .map(str::to_string)
            })
    }

    fn has_modifier(&self, node: Node, source: &str, modifier: &str) -> bool {
        let mut cursor = node.walk();
        node.children(&mut cursor).any(|child| {
            child.kind() == "modifiers" && child.utf8_text(source.as_bytes()).unwrap_or("").split_whitespace().any(|m| m == modifier)
        })
    }

    fn extract_exports(&self, root: Node, source: &str) -> Vec<Symbol> {
        let mut exports = Vec::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            let kind = match node.kind() {
                "class_declaration" | "enum_declaration" | "record_declaration" => Some(SymbolKind::Class),
                "interface_declaration" | "annotation_type_declaration" => Some(SymbolKind::Interface),
                "method_declaration" | "constructor_declaration" => Some(SymbolKind::Function),
                _ => None,
            };
            if let (Some(kind), Some(name)) = (kind, self.name(node, source)) {
                let public = self.has_modifier(node, source, "public");
                exports.push(Symbol { name, kind, exported: public, public });
            }
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
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

    fn collect_declarations(
        &self,
        path: &str,
        node: Node,
        source: &str,
        current_type: Option<&str>,
        nodes: &mut Vec<CkbNode>,
        relations: &mut Vec<TypeRelation>,
    ) {
        let mut type_ctx = current_type.map(str::to_string);
        match node.kind() {
            "class_declaration" | "record_declaration" => if let Some(name) = self.name(node, source) {
                self.push_node(path, node, NodeKind::Class, name.clone(), name.clone(), nodes);
                type_ctx = Some(name.clone());
                if let Some(superclass) = node.child_by_field_name("superclass") {
                    let target = superclass.utf8_text(source.as_bytes()).unwrap_or("").trim().trim_start_matches("extends").trim();
                    if !target.is_empty() { relations.push(TypeRelation { source_type: name.clone(), target_type: target.to_string(), kind: TypeRelationKind::Extends }); }
                }
                if let Some(interfaces) = node.child_by_field_name("interfaces") {
                    let text = interfaces.utf8_text(source.as_bytes()).unwrap_or("").trim().trim_start_matches("implements").trim();
                    for target in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                        relations.push(TypeRelation { source_type: name.clone(), target_type: target.to_string(), kind: TypeRelationKind::Implements });
                    }
                }
            },
            "interface_declaration" | "annotation_type_declaration" => if let Some(name) = self.name(node, source) {
                self.push_node(path, node, NodeKind::Interface, name.clone(), name.clone(), nodes); type_ctx = Some(name);
            },
            "enum_declaration" => if let Some(name) = self.name(node, source) {
                self.push_node(path, node, NodeKind::Enum, name.clone(), name.clone(), nodes); type_ctx = Some(name);
            },
            "method_declaration" | "constructor_declaration" => if let Some(name) = self.name(node, source) {
                let qualified = current_type.map(|t| format!("{}.{}", t, name)).unwrap_or_else(|| name.clone());
                self.push_node(path, node, NodeKind::Method, qualified, name, nodes);
            },
            _ => {}
        }
        let mut c = node.walk();
        for child in node.children(&mut c) { self.collect_declarations(path, child, source, type_ctx.as_deref(), nodes, relations); }
    }

    fn call_target(&self, node: Node, source: &str, type_ctx: Option<&str>) -> Option<String> {
        if node.kind() == "method_invocation" {
            let name = node.child_by_field_name("name").and_then(|n| n.utf8_text(source.as_bytes()).ok())?;
            let object = node.child_by_field_name("object").and_then(|n| n.utf8_text(source.as_bytes()).ok()).unwrap_or("");
            if object == "this" || object.is_empty() {
                return type_ctx.map(|t| format!("{}.{}", t, name)).or_else(|| Some(name.to_string()));
            }
            return Some(name.to_string());
        }
        if node.kind() == "object_creation_expression" {
            return node.child_by_field_name("type").and_then(|n| n.utf8_text(source.as_bytes()).ok()).map(str::to_string);
        }
        None
    }

    fn collect_calls(&self, node: Node, source: &str, type_ctx: Option<&str>, callable_ctx: Option<&str>, calls: &mut Vec<FunctionCall>) {
        let mut next_type = type_ctx.map(str::to_string);
        let mut next_callable = callable_ctx.map(str::to_string);
        if matches!(node.kind(), "class_declaration" | "interface_declaration" | "enum_declaration" | "record_declaration") {
            if let Some(name) = self.name(node, source) { next_type = Some(name); }
        } else if matches!(node.kind(), "method_declaration" | "constructor_declaration") {
            if let Some(name) = self.name(node, source) {
                next_callable = Some(next_type.as_ref().map(|t| format!("{}.{}", t, name)).unwrap_or(name));
            }
        } else if matches!(node.kind(), "method_invocation" | "object_creation_expression") {
            if let (Some(caller), Some(callee)) = (next_callable.as_ref(), self.call_target(node, source, next_type.as_deref())) {
                let p = node.start_position();
                calls.push(FunctionCall { caller_name: caller.clone(), callee_name: callee, line: (p.row + 1) as u32, column: (p.column + 1) as u32 });
            }
        }
        let mut c = node.walk();
        for child in node.children(&mut c) { self.collect_calls(child, source, next_type.as_deref(), next_callable.as_deref(), calls); }
    }
}

impl LanguageParserTrait for JavaParser {
    fn parse(&self, path: &str, content: &str) -> Result<FileAnalysis> {
        let tree = self.parser.lock().unwrap_or_else(|p| p.into_inner()).parse(content, None).ok_or_else(|| anyhow::anyhow!("Failed to parse Java file"))?;
        let root = tree.root_node();
        let imports = self.extract_imports(root, content); let exports = self.extract_exports(root, content);
        let mut nodes = Vec::new(); let mut calls = Vec::new(); let mut type_relations = Vec::new();
        let end = root.end_position(); let mut metadata = HashMap::new();
        metadata.insert("start_line".into(), "1".into()); metadata.insert("start_column".into(), "1".into());
        metadata.insert("end_line".into(), (end.row + 1).to_string()); metadata.insert("end_column".into(), (end.column + 1).to_string());
        metadata.insert("byte_start".into(), "0".into()); metadata.insert("byte_end".into(), content.len().to_string()); metadata.insert("evidence_source".into(), "tree-sitter-ast".into());
        nodes.push(CkbNode { id: NodeId(format!("{}::file", path)), kind: NodeKind::File, name: path.to_string(), path: path.into(), line: 1, column: 1, exports: exports.clone(), imports: imports.clone(), metadata });
        self.collect_declarations(path, root, content, None, &mut nodes, &mut type_relations);
        self.collect_calls(root, content, None, None, &mut calls);
        Ok(FileAnalysis { path: path.to_string(), nodes, imports, exports, calls, type_relations })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_class_method_spans_and_calls() {
        let p = JavaParser::new();
        let src = "public class Widget { public void run(){ validate(); } private void validate(){} }";
        let a = p.parse("Widget.java", src).unwrap();
        assert!(a.nodes.iter().any(|n| n.id.0 == "Widget.java::Widget" && n.kind == NodeKind::Class));
        assert!(a.nodes.iter().any(|n| n.id.0 == "Widget.java::Widget.run" && n.kind == NodeKind::Method));
        assert!(a.calls.iter().any(|c| c.caller_name == "Widget.run" && c.callee_name == "Widget.validate"));
    }

    #[test]
    fn extracts_inheritance() {
        let p = JavaParser::new();
        let a = p.parse("Child.java", "class Child extends Parent {}").unwrap();
        assert!(a.type_relations.iter().any(|r| r.source_type == "Child" && r.target_type.contains("Parent")));
    }

    #[test]
    fn handles_empty_and_malformed() {
        let p = JavaParser::new(); assert!(p.parse("Empty.java", "").is_ok()); assert!(p.parse("Broken.java", "public class {{{").is_ok());
    }
}
