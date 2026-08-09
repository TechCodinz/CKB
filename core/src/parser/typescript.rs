//! TypeScript/JavaScript parser using tree-sitter

use tree_sitter::{Node, Parser};
use crate::parser::{FileAnalysis, LanguageParserTrait};
use crate::types::{
    FunctionCall, Import, ImportKind, Node as CkbNode, NodeId, NodeKind,
    Symbol, SymbolKind, TypeRelation, TypeRelationKind,
};
use anyhow::Result;
use std::collections::HashMap;

pub struct TypeScriptParser {
    parser: std::sync::Mutex<Parser>,
}

impl TypeScriptParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_tsx())
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
                    source_str = Some(child.utf8_text(source.as_bytes()).unwrap_or("").to_string());
                }
                "import_clause" => symbols = self.extract_import_symbols(child, source),
                _ => {}
            }
        }
        source_str.map(|src| Import {
            source: src.trim_matches('"').trim_matches('\'').to_string(),
            symbols,
            kind: ImportKind::Named,
        })
    }

    fn extract_import_symbols(&self, node: Node, source: &str) -> Vec<String> {
        let mut symbols = Vec::new();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            if matches!(current.kind(), "identifier" | "type_identifier") {
                let text = current.utf8_text(source.as_bytes()).unwrap_or("");
                if !text.is_empty() {
                    symbols.push(text.to_string());
                }
            }
            let mut cursor = current.walk();
            stack.extend(current.children(&mut cursor));
        }
        symbols.sort();
        symbols.dedup();
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
                        exports.push(Symbol { name, kind: SymbolKind::Class, exported: false, public: false });
                    }
                }
                "function_declaration" => {
                    if let Some(name) = self.get_node_name(child, source) {
                        exports.push(Symbol { name, kind: SymbolKind::Function, exported: false, public: false });
                    }
                }
                "interface_declaration" => {
                    if let Some(name) = self.get_node_name(child, source) {
                        exports.push(Symbol { name, kind: SymbolKind::Interface, exported: false, public: false });
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
            let kind = match child.kind() {
                "class_declaration" => Some(SymbolKind::Class),
                "function_declaration" => Some(SymbolKind::Function),
                "interface_declaration" => Some(SymbolKind::Interface),
                "type_alias_declaration" => Some(SymbolKind::Type),
                _ => None,
            };
            if let Some(kind) = kind {
                if let Some(name) = self.get_node_name(child, source) {
                    return Some(Symbol { name, kind, exported: true, public: true });
                }
            }
        }
        None
    }

    fn get_node_name(&self, node: Node, source: &str) -> Option<String> {
        if let Some(name) = node.child_by_field_name("name") {
            let text = name.utf8_text(source.as_bytes()).unwrap_or("");
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if matches!(child.kind(), "identifier" | "type_identifier" | "property_identifier") {
                let text = child.utf8_text(source.as_bytes()).unwrap_or("");
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
        }
        None
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

    fn push_decl_node(
        &self,
        path: &str,
        node: Node,
        _source: &str,
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
        let mut class_for_children = current_class.map(str::to_string);

        match node.kind() {
            "function_declaration" => {
                if let Some(name) = self.get_node_name(node, source) {
                    self.push_decl_node(path, node, source, NodeKind::Function, name.clone(), name, nodes);
                }
            }
            "class_declaration" => {
                if let Some(name) = self.get_node_name(node, source) {
                    self.push_decl_node(path, node, source, NodeKind::Class, name.clone(), name.clone(), nodes);
                    class_for_children = Some(name.clone());

                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if matches!(child.kind(), "class_heritage" | "extends_clause") {
                            let text = child.utf8_text(source.as_bytes()).unwrap_or("");
                            for token in text.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')) {
                                if !token.is_empty() && token != "extends" && token != "implements" && token != name {
                                    relations.push(TypeRelation {
                                        source_type: name.clone(),
                                        target_type: token.to_string(),
                                        kind: TypeRelationKind::Extends,
                                    });
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            "interface_declaration" => {
                if let Some(name) = self.get_node_name(node, source) {
                    self.push_decl_node(path, node, source, NodeKind::Interface, name.clone(), name, nodes);
                }
            }
            "method_definition" | "method_signature" => {
                if let Some(method) = self.get_node_name(node, source) {
                    let qualified = current_class
                        .map(|c| format!("{}.{}", c, method))
                        .unwrap_or_else(|| method.clone());
                    self.push_decl_node(path, node, source, NodeKind::Method, qualified, method, nodes);
                }
            }
            "type_alias_declaration" => {
                if let Some(name) = self.get_node_name(node, source) {
                    self.push_decl_node(path, node, source, NodeKind::Type, name.clone(), name, nodes);
                }
            }
            "enum_declaration" => {
                if let Some(name) = self.get_node_name(node, source) {
                    self.push_decl_node(path, node, source, NodeKind::Enum, name.clone(), name, nodes);
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_declarations(path, child, source, class_for_children.as_deref(), nodes, relations);
        }
    }

    fn call_target_name(&self, call: Node, source: &str) -> Option<String> {
        let function = call.child_by_field_name("function")?;
        match function.kind() {
            "identifier" => Some(function.utf8_text(source.as_bytes()).ok()?.to_string()),
            "member_expression" | "subscript_expression" => {
                if let Some(prop) = function.child_by_field_name("property") {
                    let text = prop.utf8_text(source.as_bytes()).ok()?;
                    if !text.is_empty() {
                        return Some(text.to_string());
                    }
                }
                let mut cursor = function.walk();
                let result = function
                    .children(&mut cursor)
                    .filter(|n| matches!(n.kind(), "property_identifier" | "identifier"))
                    .last()
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                    .map(str::to_string);
                result
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

        if node.kind() == "class_declaration" {
            if let Some(name) = self.get_node_name(node, source) {
                class_ctx = Some(name);
            }
        } else if node.kind() == "function_declaration" {
            if let Some(name) = self.get_node_name(node, source) {
                callable_ctx = Some(name);
            }
        } else if matches!(node.kind(), "method_definition" | "method_signature") {
            if let Some(name) = self.get_node_name(node, source) {
                callable_ctx = Some(class_ctx.as_ref().map(|c| format!("{}.{}", c, name)).unwrap_or(name));
            }
        } else if node.kind() == "call_expression" {
            if let (Some(caller), Some(callee)) = (callable_ctx.as_ref(), self.call_target_name(node, source)) {
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

    fn parse_tree(&self, path: &str, content: &str, root: Node) -> FileAnalysis {
        let imports = self.extract_imports(root, content);
        let exports = self.extract_exports(root, content);
        let mut nodes = Vec::new();
        let mut type_relations = Vec::new();
        let mut calls = Vec::new();

        let root_end = root.end_position();
        let mut file_meta = HashMap::new();
        file_meta.insert("start_line".into(), "1".into());
        file_meta.insert("start_column".into(), "1".into());
        file_meta.insert("end_line".into(), (root_end.row + 1).to_string());
        file_meta.insert("end_column".into(), (root_end.column + 1).to_string());
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

        FileAnalysis {
            path: path.to_string(),
            nodes,
            imports,
            exports,
            calls,
            type_relations,
        }
    }
}

impl LanguageParserTrait for TypeScriptParser {
    fn parse(&self, path: &str, content: &str) -> Result<FileAnalysis> {
        let tree = self
            .parser
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse file"))?;
        Ok(self.parse_tree(path, content, tree.root_node()))
    }
}

pub struct JavaScriptParser {
    parser: std::sync::Mutex<Parser>,
}

impl JavaScriptParser {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::language_tsx())
            .expect("Failed to load JavaScript grammar");
        Self { parser: std::sync::Mutex::new(parser) }
    }
}

impl LanguageParserTrait for JavaScriptParser {
    fn parse(&self, path: &str, content: &str) -> Result<FileAnalysis> {
        let tree = self
            .parser
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse JavaScript file"))?;
        Ok(TypeScriptParser::new().parse_tree(path, content, tree.root_node()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_function_is_marked_exported() {
        let parser = TypeScriptParser::new();
        let analysis = parser.parse("hello.ts", "export function hello() {}\n").expect("parse should succeed");
        let hello = analysis.exports.iter().find(|s| s.name == "hello").expect("function should be extracted");
        assert_eq!(hello.kind, SymbolKind::Function);
        assert!(hello.exported);
        assert!(hello.public);
        assert!(analysis.nodes.iter().any(|n| n.id.0 == "hello.ts::hello" && n.kind == NodeKind::Function));
    }

    #[test]
    fn extracts_classes_methods_spans_and_calls() {
        let parser = TypeScriptParser::new();
        let source = r#"
class AuthService {
  login() { return verify(); }
}
function verify() { return true; }
"#;
        let analysis = parser.parse("auth.ts", source).unwrap();
        let method = analysis.nodes.iter().find(|n| n.id.0 == "auth.ts::AuthService.login").expect("method node");
        assert_eq!(method.kind, NodeKind::Method);
        assert!(method.metadata.get("end_line").is_some());
        assert!(analysis.calls.iter().any(|c| c.caller_name == "AuthService.login" && c.callee_name == "verify"));
    }

    #[test]
    fn non_exported_function_is_not_marked_exported() {
        let parser = TypeScriptParser::new();
        let analysis = parser.parse("internal.ts", "function helper() {}\n").expect("parse should succeed");
        let helper = analysis.exports.iter().find(|s| s.name == "helper").expect("function should be extracted");
        assert!(!helper.exported);
        assert!(!helper.public);
    }

    #[test]
    fn exported_class_is_marked_exported() {
        let parser = TypeScriptParser::new();
        let analysis = parser.parse("widget.ts", "export class Widget {}\n").expect("parse should succeed");
        let widget = analysis.exports.iter().find(|s| s.name == "Widget").expect("class should be extracted");
        assert_eq!(widget.kind, SymbolKind::Class);
        assert!(widget.exported);
    }

    #[test]
    fn handles_empty_content_without_panicking() {
        let parser = TypeScriptParser::new();
        assert!(parser.parse("empty.ts", "").is_ok());
    }

    #[test]
    fn handles_malformed_syntax_without_panicking() {
        let parser = TypeScriptParser::new();
        assert!(parser.parse("broken.ts", "export function oops( { not valid @@@ %%").is_ok());
    }
}
