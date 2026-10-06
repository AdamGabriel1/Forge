//! Reaching definitions + detecção de dead stores.
//!
//! Para cada variável, mantém o conjunto de definições (atribuições)
//! que podem alcançar cada ponto. Ao mesmo tempo, acumula quais
//! definições foram **lidas** em algum caminho.
//!
//! Ao final, `all_defs - used` são atribuições cujo valor nunca foi
//! lido — dead stores.

use crate::dataflow::Analysis;
use forge_core::Diagnostic;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

/// Identificador de uma definição (atribuição). Único por
/// `(nome, linha, coluna)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DefId {
    pub name: String,
    pub line: usize,
    pub col: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReachState {
    /// Para cada variável, definições que podem alcançar aqui.
    live: HashMap<String, HashSet<DefId>>,
    /// Definições cujo valor foi lido em algum caminho.
    pub used: HashSet<DefId>,
    /// Todas as definições vistas.
    pub all_defs: HashSet<DefId>,
}

impl ReachState {
    pub fn new() -> Self {
        Self::default()
    }
}

pub struct ReachingDefinitions<'src> {
    pub source: &'src str,
}

impl<'src> ReachingDefinitions<'src> {
    pub fn new(source: &'src str) -> Self {
        Self { source }
    }

    fn src(&self) -> &[u8] {
        self.source.as_bytes()
    }

    fn def_id(&self, name: &str, node: Node) -> DefId {
        let pos = node.start_position();
        DefId {
            name: name.to_string(),
            line: pos.row,
            col: pos.column,
            end_col: pos.column + name.len(),
        }
    }

    /// Marca como `used` todas as definições que alcançam cada leitura
    /// de variável dentro de `node`. Não desce em escopos aninhados.
    fn walk_reads<'tree>(&self, node: Node<'tree>, state: &mut ReachState) {
        match node.kind() {
            "function_definition" | "class_definition" | "lambda" => {}
            "attribute" => {
                if let Some(obj) = node.child_by_field_name("object") {
                    self.walk_reads(obj, state);
                }
            }
            "subscript" => {
                if let Some(v) = node.child_by_field_name("value") {
                    self.walk_reads(v, state);
                }
                if let Some(i) = node.child_by_field_name("subscript") {
                    self.walk_reads(i, state);
                }
            }
            "keyword_argument" => {
                if let Some(v) = node.child_by_field_name("value") {
                    self.walk_reads(v, state);
                }
            }
            "named_expression" => {
                // `(x := expr)` — o alvo não é leitura.
                if let Some(v) = node.child_by_field_name("value") {
                    self.walk_reads(v, state);
                }
            }
            "identifier" => {
                if let Ok(name) = node.utf8_text(self.src()) {
                    if let Some(defs) = state.live.get(name) {
                        let ids: Vec<DefId> = defs.iter().cloned().collect();
                        for id in ids {
                            state.used.insert(id);
                        }
                    }
                }
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.is_named() {
                        self.walk_reads(child, state);
                    }
                }
            }
        }
    }

    /// Registra `x = ...`: mata definições anteriores de `x`, adiciona
    /// a nova. `attribute`/`subscript` no LHS viram leitura.
    fn bind_def<'tree>(&self, target: Node<'tree>, state: &mut ReachState) {
        match target.kind() {
            "identifier" => {
                if let Ok(name) = target.utf8_text(self.src()) {
                    let id = self.def_id(name, target);
                    state
                        .live
                        .insert(name.to_string(), HashSet::from([id.clone()]));
                    state.all_defs.insert(id);
                }
            }
            "pattern_list"
            | "tuple_pattern"
            | "list_pattern"
            | "list_splat_pattern"
            | "dictionary_splat_pattern" => {
                let mut cursor = target.walk();
                for child in target.children(&mut cursor) {
                    if child.is_named() {
                        self.bind_def(child, state);
                    }
                }
            }
            "attribute" | "subscript" => {
                self.walk_reads(target, state);
            }
            _ => {}
        }
    }
}

impl<'src> Analysis for ReachingDefinitions<'src> {
    type State = ReachState;

    fn initial(&self) -> Self::State {
        ReachState::new()
    }

    #[allow(clippy::only_used_in_recursion)]
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
                if let Some(right) = node.child_by_field_name("right") {
                    self.walk_reads(right, &mut new);
                }
                if let Some(left) = node.child_by_field_name("left") {
                    self.bind_def(left, &mut new);
                }
            }
            "augmented_assignment" => {
                if let Some(left) = node.child_by_field_name("left") {
                    self.walk_reads(left, &mut new);
                    self.bind_def(left, &mut new);
                }
                if let Some(right) = node.child_by_field_name("right") {
                    self.walk_reads(right, &mut new);
                }
            }
            _ => {
                self.walk_reads(node, &mut new);
            }
        }

        new
    }

    fn merge(&self, a: &Self::State, b: &Self::State) -> Self::State {
        let mut new = ReachState::new();

        let mut keys: HashSet<&String> = a.live.keys().collect();
        for k in b.live.keys() {
            keys.insert(k);
        }
        for k in keys {
            let mut s: HashSet<DefId> = HashSet::new();
            if let Some(x) = a.live.get(k) {
                s.extend(x.iter().cloned());
            }
            if let Some(x) = b.live.get(k) {
                s.extend(x.iter().cloned());
            }
            new.live.insert(k.clone(), s);
        }

        new.used = a.used.union(&b.used).cloned().collect();
        new.all_defs = a.all_defs.union(&b.all_defs).cloned().collect();
        new
    }
}
