//! Python parser using tree-sitter

use tree_sitter::{Node, Parser};
use crate::parser::{FileAnalysis, LanguageParserTrait};
use crate::types::{
    FunctionCall, Import, ImportKind, Node as CkbNode, NodeId, NodeKind,
    Symbol, SymbolKind, TypeRelation, TypeRelationKind,
};
use anyhow::Result;
use std::collections::HashMap;

pub struct PythonParser {
    parser: std::sync::Mutex<Parser>,
}

impl PythonParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::language())
            .expect("Failed to load Python grammar");
        Self { parser: std::sync::Mutex::new(parser) }
    }

    fn extract_imports(&self, node: Node, source: &str) -> Vec<Import> {
        let mut imports = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "import_statement" => {
                    let mut c = child.walk();
                    for item in child.children(&mut c) {
                        if matches!(item.kind(), "dotted_name" | "aliased_import" | "identifier") {
                            let text = item.utf8_text(source.as_bytes()).unwrap_or("");
                            let package = text.split_whitespace().next().unwrap_or("");
                            if !package.is_empty() {
                                imports.push(Import {
                                    source: package.to_string(),
                                    symbols: vec![package.to_string()],
                                    kind: ImportKind::Named,
                                });
                            }
                        }
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

    fn parse_from_import(&self, node: Node, source: &str) -> Option<Import> {
        let module = if let Some(module) = node.child_by_field_name("module_name") {
            module
        } else {
            let mut cursor = node.walk();
            let found = node.children(&mut cursor)
                .find(|n| matches!(n.kind(), "dotted_name" | "relative_import" | "identifier"));
            found?
        };
        let module_name = module.utf8_text(source.as_bytes()).ok()?.to_string();
        let mut symbols = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if matches!(child.kind(), "dotted_name" | "identifier" | "aliased_import") && child.id() != module.id() {
                let text = child.utf8_text(source.as_bytes()).unwrap_or("");
                let name = text.split_whitespace().next().unwrap_or("");
                if !name.is_empty() && name != module_name {
                    symbols.push(name.to_string());
                }
            }
        }
        Some(Import { source: module_name, symbols, kind: ImportKind::Named })
    }

    fn node_name(&self, node: Node, source: &str) -> Option<String> {
        if let Some(name) = node.child_by_field_name("name") {
            let text = name.utf8_text(source.as_bytes()).unwrap_or("");
            if !text.is_empty() { return Some(text.to_string()); }
        }
        let mut cursor = node.walk();
        let result = node.children(&mut cursor)
            .find(|n| n.kind() == "identifier")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(str::to_string);
        result
    }

    fn extract_exports(&self, root: Node, source: &str) -> Vec<Symbol> {
        let mut exports = Vec::new();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            let (name, kind) = match child.kind() {
                "class_definition" => (self.node_name(child, source), Some(SymbolKind::Class)),
                "function_definition" => (self.node_name(child, source), Some(SymbolKind::Function)),
                "assignment" => {
                    let left = child.child_by_field_name("left");
                    let name = left.and_then(|n| n.utf8_text(source.as_bytes()).ok()).map(str::to_string);
                    (name, Some(SymbolKind::Variable))
                }
                _ => (None, None),
            };
            if let (Some(name), Some(kind)) = (name, kind) {
                let public = !name.starts_with('_');
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

    fn push_node(
        &self,
        path: &str,
        node: Node,
        kind: NodeKind,
        qualified_name: String,
        display_name: String,
        nodes: &mut Vec<CkbNode>,
    ) {
        let start = node.start_position();
        nodes.push(CkbNode {
            id: NodeId(format!("{}::{}", path, qualified_name)),
            kind,
            name: display_name,
            path: path.into(),
            line: (start.row + 1) as u32,
            column: (start.column + 1) as u32,
            exports: Vec::new(),
            imports: Vec::new(),
            metadata: Self::span_metadata(node),
        });
    }

    fn collect_declarations(
        &self,
        path: &str,
        node: Node,
        source: &str,
        current_class: Option<&str>,
        nodes: &mut Vec<CkbNode>,
        relations: &mut Vec<TypeRelation>,
    ) {
        let mut class_ctx = current_class.map(str::to_string);
        match node.kind() {
            "class_definition" => {
                if let Some(name) = self.node_name(node, source) {
                    self.push_node(path, node, NodeKind::Class, name.clone(), name.clone(), nodes);
                    class_ctx = Some(name.clone());
                    if let Some(superclasses) = node.child_by_field_name("superclasses") {
                        let mut cursor = superclasses.walk();
                        for child in superclasses.children(&mut cursor) {
                            if matches!(child.kind(), "identifier" | "attribute") {
                                let target = child.utf8_text(source.as_bytes()).unwrap_or("");
                                if !target.is_empty() {
                                    relations.push(TypeRelation {
                                        source_type: name.clone(),
                                        target_type: target.to_string(),
                                        kind: TypeRelationKind::Extends,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            "function_definition" => {
                if let Some(name) = self.node_name(node, source) {
                    if let Some(class) = current_class {
                        self.push_node(path, node, NodeKind::Method, format!("{}.{}", class, name), name, nodes);
                    } else {
                        self.push_node(path, node, NodeKind::Function, name.clone(), name, nodes);
                    }
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_declarations(path, child, source, class_ctx.as_deref(), nodes, relations);
        }
    }

    fn call_target(&self, call: Node, source: &str, class_ctx: Option<&str>) -> Option<String> {
        let function = call.child_by_field_name("function")?;
        match function.kind() {
            "identifier" => Some(function.utf8_text(source.as_bytes()).ok()?.to_string()),
            "attribute" => {
                let object = function.child_by_field_name("object")
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                    .unwrap_or("");
                let attr = function.child_by_field_name("attribute")
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())?;
                if object == "self" {
                    class_ctx.map(|c| format!("{}.{}", c, attr)).or_else(|| Some(attr.to_string()))
                } else {
                    Some(attr.to_string())
                }
            }
            _ => None,
        }
    }

    fn collect_calls(
        &self,
        node: Node,
        source: &str,
        current_class: Option<&str>,
        current_callable: Option<&str>,
        calls: &mut Vec<FunctionCall>,
    ) {
        let mut class_ctx = current_class.map(str::to_string);
        let mut callable_ctx = current_callable.map(str::to_string);

        if node.kind() == "class_definition" {
            if let Some(name) = self.node_name(node, source) { class_ctx = Some(name); }
        } else if node.kind() == "function_definition" {
            if let Some(name) = self.node_name(node, source) {
                callable_ctx = Some(class_ctx.as_ref().map(|c| format!("{}.{}", c, name)).unwrap_or(name));
            }
        } else if node.kind() == "call" {
            if let (Some(caller), Some(callee)) = (callable_ctx.as_ref(), self.call_target(node, source, class_ctx.as_deref())) {
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
            self.collect_calls(child, source, class_ctx.as_deref(), callable_ctx.as_deref(), calls);
        }
    }
}

impl LanguageParserTrait for PythonParser {
    fn parse(&self, path: &str, content: &str) -> Result<FileAnalysis> {
        let tree = self
            .parser
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse Python file"))?;

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
            id: NodeId(format!("{}::file", path)),
            kind: NodeKind::File,
            name: path.to_string(),
            path: path.into(),
            line: 1,
            column: 1,
            exports: exports.clone(),
            imports: imports.clone(),
            metadata: file_meta,
        });

        self.collect_declarations(path, root, content, None, &mut nodes, &mut type_relations);
        self.collect_calls(root, content, None, None, &mut calls);

        Ok(FileAnalysis {
            path: path.to_string(),
            nodes,
            imports,
            exports,
            calls,
            type_relations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_top_level_function_as_graph_node() {
        let parser = PythonParser::new();
        let analysis = parser.parse("hello.py", "def hello():\n    pass\n").unwrap();
        assert!(analysis.exports.iter().any(|s| s.name == "hello" && s.kind == SymbolKind::Function));
        let node = analysis.nodes.iter().find(|n| n.id.0 == "hello.py::hello").expect("function graph node");
        assert_eq!(node.line, 1);
        assert!(node.metadata.get("end_line").is_some());
    }

    #[test]
    fn extracts_class_methods_and_self_calls() {
        let parser = PythonParser::new();
        let src = "class Widget:\n    def save(self):\n        return self.validate()\n    def validate(self):\n        return True\n";
        let analysis = parser.parse("widget.py", src).unwrap();
        assert!(analysis.nodes.iter().any(|n| n.id.0 == "widget.py::Widget.save" && n.kind == NodeKind::Method));
        assert!(analysis.calls.iter().any(|c| c.caller_name == "Widget.save" && c.callee_name == "Widget.validate"));
    }

    #[test]
    fn extracts_imports() {
        let parser = PythonParser::new();
        let analysis = parser.parse("main.py", "import os\nfrom collections import OrderedDict\n").unwrap();
        assert!(analysis.imports.iter().any(|i| i.source == "os"));
        assert!(analysis.imports.iter().any(|i| i.source == "collections"));
    }

    #[test]
    fn private_python_symbols_are_not_public_exports() {
        let parser = PythonParser::new();
        let analysis = parser.parse("internal.py", "def _helper():\n    pass\n").unwrap();
        let helper = analysis.exports.iter().find(|s| s.name == "_helper").unwrap();
        assert!(!helper.public);
        assert!(!helper.exported);
    }

    #[test]
    fn handles_empty_content_without_panicking() {
        let parser = PythonParser::new();
        assert!(parser.parse("empty.py", "").is_ok());
    }

    #[test]
    fn handles_malformed_syntax_without_panicking() {
        let parser = PythonParser::new();
        assert!(parser.parse("broken.py", "def oops(:\n    this is not : valid python @@@").is_ok());
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
