//! Análise de atribuição definida: rastreia se uma variável local foi
//! atribuída em **todos** os caminhos até um dado ponto.
//!
//! Estado: mapa `nome -> bool` onde `true` = definitivamente atribuída.
//!
//! - Atribuir marca `true`.
//! - `if` sem `else`: o merge entre (ramos then) e (entrada) rebaixa
//!   para `false` se a atribuição só ocorre no `then`.
//! - Uso de um nome local que está `false` → diagnóstico.

use crate::dataflow::Analysis;
use crate::range_of;
use forge_core::{Diagnostic, Severity};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssignState {
    map: HashMap<String, bool>,
}

impl AssignState {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn get(&self, name: &str) -> bool {
        self.map.get(name).copied().unwrap_or(false)
    }

    pub fn set(&mut self, name: &str, assigned: bool) {
        self.map.insert(name.to_string(), assigned);
    }

    fn mark_all(&mut self, names: &HashSet<String>) {
        for n in names {
            self.map.insert(n.clone(), true);
        }
    }
}

pub struct DefiniteAssignmentAnalysis<'src> {
    pub source: &'src str,
    /// Nomes que são "locais" deste escopo — só eles são candidatos a
    /// diagnóstico. Globais, imports e builtins ficam de fora.
    pub locals: HashSet<String>,
    /// Parâmetros — já atribuídos no início da função.
    pub params: HashSet<String>,
}

impl<'src> DefiniteAssignmentAnalysis<'src> {
    pub fn new(
        source: &'src str,
        locals: HashSet<String>,
        params: HashSet<String>,
    ) -> Self {
        Self {
            source,
            locals,
            params,
        }
    }

    fn src(&self) -> &[u8] {
        self.source.as_bytes()
    }

    /// Percorre uma sub-expressão em busca de identificadores em posição
    /// de leitura, sinalizando os que não estão definitivamente atribuídos.
    /// **Não desce** em escopos aninhados.
    fn walk_reads<'tree>(
        &self,
        node: Node<'tree>,
        state: &AssignState,
        diags: &mut Vec<Diagnostic>,
    ) {
        match node.kind() {
            "function_definition" | "class_definition" | "lambda" => {}
            "attribute" => {
                // `obj.attr`: só `obj` é leitura. `attr` não é variável.
                if let Some(obj) = node.child_by_field_name("object") {
                    self.walk_reads(obj, state, diags);
                }
            }
            "subscript" => {
                if let Some(v) = node.child_by_field_name("value") {
                    self.walk_reads(v, state, diags);
                }
                if let Some(i) = node.child_by_field_name("subscript") {
                    self.walk_reads(i, state, diags);
                }
            }
            "keyword_argument" => {
                // `f(x=1)` — o nome do parâmetro (`x`) não é uma variável.
                if let Some(v) = node.child_by_field_name("value") {
                    self.walk_reads(v, state, diags);
                }
            }
            "identifier" => {
                let Ok(name) = node.utf8_text(self.src()) else {
                    return;
                };
                if !self.locals.contains(name) {
                    return;
                }
                if state.get(name) {
                    return;
                }
                diags.push(Diagnostic::new(
                    "FOR012",
                    &format!(
                        "`{}` é usado antes de ser atribuído em todos os caminhos — `UnboundLocalError`.",
                        name
                    ),
                    range_of(node),
                    Severity::Warning,
                ));
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.is_named() {
                        self.walk_reads(child, state, diags);
                    }
                }
            }
        }
    }

    /// Registra um alvo (LHS) como atribuído. Se o alvo é um `attribute`
    /// ou `subscript`, o objeto subjacente é uma leitura.
    fn bind_target<'tree>(
        &self,
        node: Node<'tree>,
        state: &mut AssignState,
        diags: &mut Vec<Diagnostic>,
    ) {
        match node.kind() {
            "identifier" => {
                if let Ok(name) = node.utf8_text(self.src()) {
                    state.set(name, true);
                }
            }
            "pattern_list" | "tuple_pattern" | "list_pattern"
            | "list_splat_pattern" | "dictionary_splat_pattern" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.is_named() {
                        self.bind_target(child, state, diags);
                    }
                }
            }
            "attribute" | "subscript" => {
                // `x.y = 1` — `x` é leitura.
                self.walk_reads(node, state, diags);
            }
            _ => {}
        }
    }
}

impl<'src> Analysis for DefiniteAssignmentAnalysis<'src> {
    type State = AssignState;

    fn initial(&self) -> Self::State {
        let mut s = AssignState::new();
        s.mark_all(&self.params);
        s
    }

    fn transfer<'tree>(
        &self,
        node: Node<'tree>,
        state: &Self::State,
        diags: &mut Vec<Diagnostic>,
    ) -> Self::State {
        let mut new = state.clone();

        match node.kind() {
            "expression_statement" => {
                if let Some(inner) = node.named_child(0) {
                    return self.transfer(inner, state, diags);
                }
            }
            "assignment" => {
                // RHS é avaliado primeiro.
                if let Some(right) = node.child_by_field_name("right") {
                    self.walk_reads(right, state, diags);
                }
                if let Some(left) = node.child_by_field_name("left") {
                    self.bind_target(left, &mut new, diags);
                }
            }
            "augmented_assignment" => {
                // `x += 1` — LHS é lido e depois reatribuído.
                if let Some(left) = node.child_by_field_name("left") {
                    self.walk_reads(left, state, diags);
                    self.bind_target(left, &mut new, diags);
                }
                if let Some(right) = node.child_by_field_name("right") {
                    self.walk_reads(right, state, diags);
                }
            }
            _ => {
                // Qualquer outro statement é lido puro.
                self.walk_reads(node, state, diags);
            }
        }

        new
    }

    fn merge(&self, a: &Self::State, b: &Self::State) -> Self::State {
        let mut new = AssignState::new();
        let mut keys: Vec<&String> = a.map.keys().collect();
        for k in b.map.keys() {
            if !a.map.contains_key(k) {
                keys.push(k);
            }
        }
        for k in keys {
            // AND lógico: só é definitivamente atribuída se **ambos** os
            // caminhos atribuíram.
            new.set(k, a.get(k) && b.get(k));
        }
        new
    }
}

/// Coleta nomes "locais" e "parâmetros" de uma `function_definition`.
///
/// Locals = tudo que aparece como alvo de atribuição simples ou walrus
/// (`:=`) no corpo. Não inclui alvos de `for`, `with`, `except` nem
/// compreensões — fica para uma segunda passada, quando cobrirmos esses
/// casos no motor.
pub fn collect_locals(
    func_def: Node,
    source: &str,
) -> (HashSet<String>, HashSet<String>) {
    let mut locals = HashSet::new();
    let mut params = HashSet::new();

    if let Some(p) = func_def.child_by_field_name("parameters") {
        collect_params(p, source, &mut params);
    }
    if let Some(body) = func_def.child_by_field_name("body") {
        walk_collect(body, source, &mut locals);
    }

    (locals, params)
}

fn collect_params(params: Node, source: &str, out: &mut HashSet<String>) {
    let mut cursor = params.walk();
    for child in params.children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                if let Ok(n) = child.utf8_text(source.as_bytes()) {
                    out.insert(n.to_string());
                }
            }
            "typed_parameter" | "default_parameter" | "typed_default_parameter"
            | "list_splat_pattern" | "dictionary_splat_pattern" => {
                let mut c = child.walk();
                for sub in child.children(&mut c) {
                    if sub.kind() == "identifier" {
                        if let Ok(n) = sub.utf8_text(source.as_bytes()) {
                            out.insert(n.to_string());
                        }
                        break;
                    }
                }
            }
            _ => {}
        }
    }
}

fn walk_collect(node: Node, source: &str, locals: &mut HashSet<String>) {
    match node.kind() {
        "function_definition" | "class_definition" | "lambda" => return,
        "assignment" | "augmented_assignment" => {
            if let Some(left) = node.child_by_field_name("left") {
                collect_target_names(left, source, locals);
            }
        }
        "named_expression" => {
            if let Some(name) = node.child_by_field_name("name") {
                if let Ok(n) = name.utf8_text(source.as_bytes()) {
                    locals.insert(n.to_string());
                }
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_named() {
            walk_collect(child, source, locals);
        }
    }
}

fn collect_target_names(node: Node, source: &str, out: &mut HashSet<String>) {
    match node.kind() {
        "identifier" => {
            if let Ok(n) = node.utf8_text(source.as_bytes()) {
                out.insert(n.to_string());
            }
        }
        "pattern_list" | "tuple_pattern" | "list_pattern"
        | "list_splat_pattern" | "dictionary_splat_pattern" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    collect_target_names(child, source, out);
                }
            }
        }
        _ => {}
    }
}
