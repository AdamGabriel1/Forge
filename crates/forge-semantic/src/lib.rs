use forge_core::Range;
use std::collections::HashMap;
use tree_sitter::Node;

// ---------------------------------------------------------------------------
// Tipos públicos
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Module,
    Function,
    Class,
    Lambda,
    Comprehension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKind {
    Assignment,
    Parameter,
    Function,
    Class,
    ForTarget,
    WithTarget,
    ExceptTarget,
    Walrus,
    Import,
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub name: String,
    pub range: Range,
    pub kind: BindingKind,
    pub used: bool,
}

#[derive(Debug)]
pub struct Scope {
    pub kind: ScopeKind,
    pub parent: Option<usize>,
    pub range: Range,
    pub bindings: HashMap<String, Binding>,
    pub has_wildcard_import: bool,
}

#[derive(Debug, Clone)]
struct Use {
    name: String,
    scope: usize,
    range: Range,
}

#[derive(Debug)]
pub struct SemanticModel {
    pub scopes: Vec<Scope>,
    uses: Vec<Use>,
}

impl SemanticModel {
    pub fn analyze(root: Node, source: &str) -> Self {
        let mut analyzer = Analyzer {
            scopes: Vec::new(),
            uses: Vec::new(),
        };
        let module_id = analyzer.push_scope(ScopeKind::Module, range_of(root), None);
        analyzer.visit_body(root, module_id, source);
        analyzer.resolve();

        SemanticModel {
            scopes: analyzer.scopes,
            uses: analyzer.uses,
        }
    }

    pub fn bindings(&self) -> impl Iterator<Item = (&Scope, &Binding)> {
        self.scopes
            .iter()
            .flat_map(|scope| scope.bindings.values().map(move |b| (scope, b)))
    }

    /// Retorna os usos que **não** resolvem para nenhum binding na cadeia
    /// de escopos e cujo escopo (ou ancestral) não tenha `import *`.
    pub fn unresolved_uses(&self) -> Vec<(&str, &Range)> {
        let mut out = Vec::new();
        for u in &self.uses {
            if self.has_wildcard_in_chain(u.scope) {
                continue;
            }
            if self.resolve_name(&u.name, u.scope).is_none() {
                out.push((u.name.as_str(), &u.range));
            }
        }
        out
    }

    fn resolve_name(&self, name: &str, from_scope: usize) -> Option<usize> {
        let mut current = Some(from_scope);
        while let Some(sid) = current {
            if self.scopes[sid].bindings.contains_key(name) {
                return Some(sid);
            }
            current = self.scopes[sid].parent;
        }
        None
    }

    fn has_wildcard_in_chain(&self, from_scope: usize) -> bool {
        let mut current = Some(from_scope);
        while let Some(sid) = current {
            let scope = &self.scopes[sid];
            if scope.has_wildcard_import {
                return true;
            }
            current = scope.parent;
        }
        false
    }
    /// Para cada uso resolvido, retorna `(nome, range, escopo_da_use, escopo_resolvido)`.
    ///
    /// `escopo_resolvido` é o escopo onde o binding foi efetivamente
    /// encontrado, subindo a cadeia de pais. Usado por `FOR012` para
    /// distinguir "uso antes de definição no mesmo escopo" de
    /// "captura de closure" (que é lazy em Python).
    pub fn resolved_uses(&self) -> Vec<(&str, &Range, usize, usize)> {
        let mut out = Vec::new();
        for u in &self.uses {
            if let Some(resolved) = self.resolve_name(&u.name, u.scope) {
                out.push((u.name.as_str(), &u.range, u.scope, resolved));
            }
        }
        out
    }
}

fn range_of(node: Node) -> Range {
    let start = node.start_position();
    let end = node.end_position();
    Range {
        start_line: start.row,
        start_col: start.column,
        end_line: end.row,
        end_col: end.column,
    }
}

fn first_child_of_kind<'a>(parent: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = parent.walk();
    for child in parent.children(&mut cursor) {
        if child.kind() == kind {
            return Some(child);
        }
    }
    None
}

fn first_segment(node: Node, source: &str) -> String {
    match node.utf8_text(source.as_bytes()) {
        Ok(text) => text.split('.').next().unwrap_or(text).to_string(),
        Err(_) => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Analisador
// ---------------------------------------------------------------------------

struct Analyzer {
    scopes: Vec<Scope>,
    uses: Vec<Use>,
}

impl Analyzer {
    fn push_scope(&mut self, kind: ScopeKind, range: Range, parent: Option<usize>) -> usize {
        let id = self.scopes.len();
        self.scopes.push(Scope {
            kind,
            parent,
            range,
            bindings: HashMap::new(),
            has_wildcard_import: false,
        });
        id
    }

    fn record_binding(&mut self, node: Node, scope: usize, kind: BindingKind, source: &str) {
        let Ok(name) = node.utf8_text(source.as_bytes()) else {
            return;
        };
        self.record_binding_named(name, range_of(node), scope, kind);
    }

    fn record_binding_named(
        &mut self,
        name: &str,
        range: Range,
        scope: usize,
        kind: BindingKind,
    ) {
        self.scopes[scope]
            .bindings
            .entry(name.to_string())
            .or_insert_with(|| Binding {
                name: name.to_string(),
                range,
                kind,
                used: false,
            });
    }

    fn record_use(&mut self, node: Node, scope: usize, source: &str) {
        let Ok(name) = node.utf8_text(source.as_bytes()) else {
            return;
        };
        self.uses.push(Use {
            name: name.to_string(),
            scope,
            range: range_of(node),
        });
    }

    fn resolve(&mut self) {
        // Itera sobre uma cópia dos índices para evitar borrow conflict.
        let uses: Vec<(String, usize)> =
            self.uses.iter().map(|u| (u.name.clone(), u.scope)).collect();
        for (name, scope) in uses {
            let mut current = Some(scope);
            while let Some(sid) = current {
                if self.scopes[sid].bindings.contains_key(&name) {
                    if let Some(b) = self.scopes[sid].bindings.get_mut(&name) {
                        b.used = true;
                    }
                    break;
                }
                current = self.scopes[sid].parent;
            }
        }
    }

    // -- Traversal --------------------------------------------------------

    fn visit_body(&mut self, node: Node, scope: usize, source: &str) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child, scope, source);
        }
    }

    fn visit_node(&mut self, node: Node, scope: usize, source: &str) {
        match node.kind() {
            "function_definition" => self.visit_function(node, scope, source),
            "class_definition" => self.visit_class(node, scope, source),
            "lambda" => self.visit_lambda(node, scope, source),
            "assignment" => self.visit_assignment(node, scope, source),
            "augmented_assignment" => self.visit_augmented(node, scope, source),
            "for_statement" => self.visit_for(node, scope, source),
            "with_statement" => self.visit_with(node, scope, source),
            "with_item" => self.visit_with_item(node, scope, source),
            "except_clause" => self.visit_except(node, scope, source),
            "named_expression" => self.visit_walrus(node, scope, source),
            "import_statement" | "import_from_statement" => {
                self.visit_import(node, scope, source)
            }
            "list_comprehension"
            | "set_comprehension"
            | "dictionary_comprehension"
            | "generator_expression" => self.visit_comprehension(node, scope, source),
            "identifier" => self.record_use(node, scope, source),
            // `obj.atributo`: só `obj` é um uso de nome. O `atributo`
            // é resolvido dinamicamente em runtime e não é alvo de
            // `undefined_name`. Mesmo raciocínio para `subscript`.
            "attribute" => {
                if let Some(object) = node.child_by_field_name("object") {
                    self.visit_node(object, scope, source);
                }
            }
            "subscript" => {
                if let Some(value) = node.child_by_field_name("value") {
                    self.visit_node(value, scope, source);
                }
                if let Some(index) = node.child_by_field_name("subscript") {
                    self.visit_node(index, scope, source);
                }
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(child, scope, source);
                }
            }
        }
    }

    fn visit_function(&mut self, node: Node, scope: usize, source: &str) {
        if let Some(name) = node.child_by_field_name("name") {
            self.record_binding(name, scope, BindingKind::Function, source);
        }
        let func_scope = self.push_scope(ScopeKind::Function, range_of(node), Some(scope));
        if let Some(params) = node.child_by_field_name("parameters") {
            self.bind_parameters(params, func_scope, source);
        }
        if let Some(body) = node.child_by_field_name("body") {
            self.visit_body(body, func_scope, source);
        }
    }

    fn bind_parameters(&mut self, params: Node, scope: usize, source: &str) {
        let mut cursor = params.walk();
        for child in params.children(&mut cursor) {
            match child.kind() {
                "identifier" => {
                    self.record_binding(child, scope, BindingKind::Parameter, source);
                }
                "typed_parameter" => {
                    if let Some(id) = first_child_of_kind(child, "identifier") {
                        self.record_binding(id, scope, BindingKind::Parameter, source);
                    }
                }
                "default_parameter" | "typed_default_parameter" => {
                    if let Some(id) = child.child_by_field_name("name") {
                        self.record_binding(id, scope, BindingKind::Parameter, source);
                    }
                }
                "list_splat_pattern" | "dictionary_splat_pattern" => {
                    if let Some(id) = first_child_of_kind(child, "identifier") {
                        self.record_binding(id, scope, BindingKind::Parameter, source);
                    }
                }
                _ => {}
            }
        }
    }

    fn visit_class(&mut self, node: Node, scope: usize, source: &str) {
        if let Some(name) = node.child_by_field_name("name") {
            self.record_binding(name, scope, BindingKind::Class, source);
        }
        let class_scope = self.push_scope(ScopeKind::Class, range_of(node), Some(scope));
        if let Some(body) = node.child_by_field_name("body") {
            self.visit_body(body, class_scope, source);
        }
    }

    fn visit_lambda(&mut self, node: Node, scope: usize, source: &str) {
        let lambda_scope = self.push_scope(ScopeKind::Lambda, range_of(node), Some(scope));
        if let Some(params) = node.child_by_field_name("parameters") {
            self.bind_parameters(params, lambda_scope, source);
        }
        if let Some(body) = node.child_by_field_name("body") {
            self.visit_node(body, lambda_scope, source);
        }
    }

    fn visit_assignment(&mut self, node: Node, scope: usize, source: &str) {
        if let Some(left) = node.child_by_field_name("left") {
            self.bind_target(left, scope, BindingKind::Assignment, source);
        }
        if let Some(right) = node.child_by_field_name("right") {
            self.visit_node(right, scope, source);
        }
    }

    fn visit_augmented(&mut self, node: Node, scope: usize, source: &str) {
        if let Some(left) = node.child_by_field_name("left") {
            self.visit_node(left, scope, source);
        }
        if let Some(right) = node.child_by_field_name("right") {
            self.visit_node(right, scope, source);
        }
    }

    fn visit_for(&mut self, node: Node, scope: usize, source: &str) {
        if let Some(left) = node.child_by_field_name("left") {
            self.bind_target(left, scope, BindingKind::ForTarget, source);
        }
        if let Some(right) = node.child_by_field_name("right") {
            self.visit_node(right, scope, source);
        }
        if let Some(body) = node.child_by_field_name("body") {
            self.visit_body(body, scope, source);
        }
    }

    fn visit_with(&mut self, node: Node, scope: usize, source: &str) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "with_clause" => {
                    let mut c = child.walk();
                    for item in child.children(&mut c) {
                        if item.kind() == "with_item" {
                            self.visit_with_item(item, scope, source);
                        }
                    }
                }
                "with_item" => self.visit_with_item(child, scope, source),
                "block" => self.visit_body(child, scope, source),
                _ => {}
            }
        }
    }

    fn visit_with_item(&mut self, node: Node, scope: usize, source: &str) {
        let mut seen_as = false;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "as" => seen_as = true,
                "as_pattern" => {
                    self.handle_as_pattern(child, scope, BindingKind::WithTarget, source);
                }
                _ if seen_as && child.is_named() => {
                    self.bind_target(child, scope, BindingKind::WithTarget, source);
                    seen_as = false;
                }
                _ if child.is_named() => self.visit_node(child, scope, source),
                _ => {}
            }
        }
    }

    fn visit_except(&mut self, node: Node, scope: usize, source: &str) {
        let mut seen_as = false;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "as" => seen_as = true,
                "as_pattern" => {
                    self.handle_as_pattern(child, scope, BindingKind::ExceptTarget, source);
                }
                "block" => self.visit_body(child, scope, source),
                _ if seen_as && child.is_named() => {
                    self.bind_target(child, scope, BindingKind::ExceptTarget, source);
                    seen_as = false;
                }
                _ if child.is_named() => self.visit_node(child, scope, source),
                _ => {}
            }
        }
    }

    fn handle_as_pattern(&mut self, node: Node, scope: usize, kind: BindingKind, source: &str) {
        let mut seen_as = false;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "as" => seen_as = true,
                _ if seen_as && child.is_named() => {
                    self.bind_target(child, scope, kind, source);
                    seen_as = false;
                }
                _ if child.is_named() => self.visit_node(child, scope, source),
                _ => {}
            }
        }
    }

    fn visit_walrus(&mut self, node: Node, scope: usize, source: &str) {
        if let Some(name) = node.child_by_field_name("name") {
            self.record_binding(name, scope, BindingKind::Walrus, source);
        }
        if let Some(value) = node.child_by_field_name("value") {
            self.visit_node(value, scope, source);
        } else {
            let mut cursor = node.walk();
            let mut first_id_seen = false;
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier" && !first_id_seen {
                    first_id_seen = true;
                    self.record_binding(child, scope, BindingKind::Walrus, source);
                } else if child.is_named() {
                    self.visit_node(child, scope, source);
                }
            }
        }
    }

    /// `import os`, `import os.path`, `import os as o`,
    /// `from x import y`, `from x import y as z`, `from x import *`.
    fn visit_import(&mut self, node: Node, scope: usize, source: &str) {
        match node.kind() {
            "import_statement" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    match child.kind() {
                        "dotted_name" => {
                            let name = first_segment(child, source);
                            if !name.is_empty() {
                                self.record_binding_named(
                                    &name,
                                    range_of(child),
                                    scope,
                                    BindingKind::Import,
                                );
                            }
                        }
                        "aliased_import" => {
                            if let Some(alias) = child.child_by_field_name("alias") {
                                self.record_binding(
                                    alias,
                                    scope,
                                    BindingKind::Import,
                                    source,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            "import_from_statement" => {
                let mut seen_module = false;
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    match child.kind() {
                        "relative_import" => seen_module = true,
                        "dotted_name" => {
                            if !seen_module {
                                seen_module = true;
                            } else {
                                let name = first_segment(child, source);
                                if !name.is_empty() {
                                    self.record_binding_named(
                                        &name,
                                        range_of(child),
                                        scope,
                                        BindingKind::Import,
                                    );
                                }
                            }
                        }
                        "aliased_import" => {
                            if let Some(alias) = child.child_by_field_name("alias") {
                                self.record_binding(
                                    alias,
                                    scope,
                                    BindingKind::Import,
                                    source,
                                );
                            }
                        }
                        "wildcard_import" => {
                            self.scopes[scope].has_wildcard_import = true;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn visit_comprehension(&mut self, node: Node, scope: usize, source: &str) {
        let comp_scope = self.push_scope(ScopeKind::Comprehension, range_of(node), Some(scope));
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "for_in_clause" {
                if let Some(left) = child.child_by_field_name("left") {
                    self.bind_target(left, comp_scope, BindingKind::ForTarget, source);
                }
                if let Some(right) = child.child_by_field_name("right") {
                    self.visit_node(right, comp_scope, source);
                }
            } else {
                self.visit_node(child, comp_scope, source);
            }
        }
    }

    fn bind_target(&mut self, node: Node, scope: usize, kind: BindingKind, source: &str) {
        match node.kind() {
            "identifier" => self.record_binding(node, scope, kind, source),
            "pattern_list" | "tuple_pattern" | "list_pattern" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.is_named() {
                        self.bind_target(child, scope, kind, source);
                    }
                }
            }
            "list_splat_pattern" | "dictionary_splat_pattern" | "as_pattern_target" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.is_named() {
                        self.bind_target(child, scope, kind, source);
                    }
                }
            }
            "attribute" | "subscript" => {
                self.visit_node(node, scope, source);
            }
            _ => {
                self.visit_node(node, scope, source);
            }
        }
    }
}