//! Análise de nulidade: rastreia se uma variável pode ser `None`.
//!
//! Lattice (de baixo para cima):
//!
//! ```text
//!       Unknown (⊤)
//!          |
//!       MaybeNone
//!       /       \
//!  NotNone   DefinitelyNone
//!       \       /
//!         ⊥ (nunca observado)
//! ```
//!
//! - `NotNone` — sabemos que não é None.
//! - `DefinitelyNone` — sabemos que é None.
//! - `MaybeNone` — pode ser qualquer coisa (merge de caminhos).
//! - `Unknown` — não temos informação (parâmetro, valor externo).

use crate::dataflow::Analysis;
use crate::range_of;
use forge_core::{Diagnostic, Severity};
use std::collections::HashMap;
use tree_sitter::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nullable {
    NotNone,
    DefinitelyNone,
    MaybeNone,
    Unknown,
}

impl Nullable {
    fn join(self, other: Nullable) -> Nullable {
        use Nullable::*;
        match (self, other) {
            (a, b) if a == b => a,
            (Unknown, _) | (_, Unknown) => Unknown,
            _ => MaybeNone,
        }
    }

    /// `true` se desreferenciar uma variável nesse estado deve ser reportado.
    pub fn is_suspicious(self) -> bool {
        matches!(self, Nullable::DefinitelyNone | Nullable::MaybeNone)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NullableState {
    map: HashMap<String, Nullable>,
}

impl NullableState {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn get(&self, name: &str) -> Nullable {
        self.map.get(name).copied().unwrap_or(Nullable::Unknown)
    }

    pub fn set(&mut self, name: &str, value: Nullable) {
        self.map.insert(name.to_string(), value);
    }
}

pub struct NullableAnalysis<'src> {
    pub source: &'src str,
}

impl<'src> NullableAnalysis<'src> {
    pub fn new(source: &'src str) -> Self {
        Self { source }
    }

    fn src(&self) -> &[u8] {
        self.source.as_bytes()
    }

    /// Percorre uma sub-expressão procurando derefs (`x.foo`, `x[i]`)
    /// que sejam suspeitos dado o `state`. **Não desce** em escopos
    /// aninhados (`def`, `class`, `lambda`) — eles são tratados
    /// separadamente pelo `check` da regra.
    fn walk_expr<'tree>(
        &self,
        node: Node<'tree>,
        state: &NullableState,
        diags: &mut Vec<Diagnostic>,
    ) {
        let bytes = self.src();
        match node.kind() {
            "function_definition" | "class_definition" | "lambda" => {
                // Escopos separados.
            }
            "attribute" => {
                if let Some(obj) = node.child_by_field_name("object") {
                    check_deref(obj, state, bytes, "atributo", diags);
                    self.walk_expr(obj, state, diags);
                }
            }
            "subscript" => {
                if let Some(value) = node.child_by_field_name("value") {
                    check_deref(value, state, bytes, "índice", diags);
                    self.walk_expr(value, state, diags);
                }
                if let Some(idx) = node.child_by_field_name("subscript") {
                    self.walk_expr(idx, state, diags);
                }
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.is_named() {
                        self.walk_expr(child, state, diags);
                    }
                }
            }
        }
    }
}

impl<'src> Analysis for NullableAnalysis<'src> {
    type State = NullableState;

    fn initial(&self) -> Self::State {
        NullableState::new()
    }

    fn transfer<'tree>(
        &self,
        node: Node<'tree>,
        state: &Self::State,
        diags: &mut Vec<Diagnostic>,
    ) -> Self::State {
        let mut new = state.clone();
        let bytes = self.src();

        match node.kind() {
            "assignment" => {
                // RHS é avaliado primeiro em Python — derefs lá usam o
                // estado de entrada.
                if let Some(right) = node.child_by_field_name("right") {
                    self.walk_expr(right, state, diags);
                }
                if let Some(left) = node.child_by_field_name("left") {
                    if left.kind() == "identifier" {
                        if let Ok(name) = left.utf8_text(bytes) {
                            let value = node
                                .child_by_field_name("right")
                                .map(|r| classify_value(r, bytes))
                                .unwrap_or(Nullable::Unknown);
                            new.set(name, value);
                        }
                    } else {
                        // `x.y = 5` / `x[i] = 5` — LHS é uso.
                        self.walk_expr(left, state, diags);
                    }
                }
            }
            _ => {
                // Qualquer outro nó: procura derefs nas sub-expressões.
                self.walk_expr(node, state, diags);
            }
        }

        new
    }

    fn refine<'tree>(
        &self,
        cond: Node<'tree>,
        state: &Self::State,
        positive: bool,
    ) -> Self::State {
        let mut new = state.clone();
        let bytes = self.src();

        if let Some((name, checks_for_none)) = parse_none_check(cond, bytes) {
            let target = if checks_for_none == positive {
                Nullable::DefinitelyNone
            } else {
                Nullable::NotNone
            };
            new.set(name, target);
        }

        new
    }

    fn merge(&self, a: &Self::State, b: &Self::State) -> Self::State {
        let mut new = NullableState::new();
        let mut keys: Vec<&String> = a.map.keys().collect();
        for k in b.map.keys() {
            if !a.map.contains_key(k) {
                keys.push(k);
            }
        }
        for k in keys {
            new.set(k, a.get(k).join(b.get(k)));
        }
        new
    }
}

fn check_deref(
    target: Node,
    state: &NullableState,
    bytes: &[u8],
    kind_label: &str,
    diags: &mut Vec<Diagnostic>,
) {
    if target.kind() != "identifier" {
        return;
    }
    let Ok(name) = target.utf8_text(bytes) else {
        return;
    };
    let value = state.get(name);
    if value.is_suspicious() {
        let msg = match value {
            Nullable::DefinitelyNone => format!(
                "`{}` é `None` ao ser desreferenciado por {} — `TypeError` em runtime.",
                name, kind_label
            ),
            Nullable::MaybeNone => format!(
                "`{}` pode ser `None` ao ser desreferenciado por {} (depende do caminho de execução).",
                name, kind_label
            ),
            _ => return,
        };
        diags.push(Diagnostic::new(
            "FOR013",
            &msg,
            range_of(target),
            Severity::Warning,
        ));
    }
}

/// Classifica uma expressão RHS. `None` literal → `DefinitelyNone`;
/// qualquer outra coisa → `NotNone` (por ora — não rastreamos valores).
fn classify_value(node: Node, bytes: &[u8]) -> Nullable {
    if node.kind() == "none" {
        return Nullable::DefinitelyNone;
    }
    // `x = y` — propaga? Poderia, mas deixamos como Unknown por simplicidade.
    if node.kind() == "identifier" {
        let _ = node.utf8_text(bytes);
        return Nullable::Unknown;
    }
    Nullable::NotNone
}

/// Detecta `x is None`, `x is not None`, `x == None`, `x != None`
/// (e as variantes com `None` à esquerda).
///
/// Retorna `(nome, checks_for_none)`. Se `checks_for_none` é `true`, a
/// condição pergunta se `x` é `None`.
fn parse_none_check<'tree>(
    cond: Node<'tree>,
    bytes: &'tree [u8],
) -> Option<(&'tree str, bool)> {
    let mut cursor = cond.walk();
    let children: Vec<Node> = cond.children(&mut cursor).collect();

    let first = children.iter().find(|c| c.is_named())?;
    let last = children.iter().rev().find(|c| c.is_named())?;

    let (id_node, _op_side) = if first.kind() == "none" {
        (last, 0)
    } else if last.kind() == "none" {
        (first, 1)
    } else {
        return None;
    };

    if id_node.kind() != "identifier" {
        return None;
    }

    let mut op_parts: Vec<&str> = Vec::new();
    for child in &children {
        if child.is_named() {
            continue;
        }
        if let Ok(t) = child.utf8_text(bytes) {
            if t == "is" || t == "not" || t == "==" || t == "!=" {
                op_parts.push(t);
            }
        }
    }

    let op = op_parts.join(" ");
    let is_not = op == "is not" || op == "!=";
    let is_eq = op == "is" || op == "==";
    if !is_not && !is_eq {
        return None;
    }

    let name = id_node.utf8_text(bytes).ok()?;

    Some((name, !is_not))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lattice_join() {
        use Nullable::*;
        assert_eq!(NotNone.join(NotNone), NotNone);
        assert_eq!(DefinitelyNone.join(DefinitelyNone), DefinitelyNone);
        assert_eq!(NotNone.join(DefinitelyNone), MaybeNone);
        assert_eq!(DefinitelyNone.join(NotNone), MaybeNone);
        assert_eq!(MaybeNone.join(NotNone), MaybeNone);
        assert_eq!(Unknown.join(NotNone), Unknown);
        assert_eq!(Unknown.join(DefinitelyNone), Unknown);
    }

    #[test]
    fn suspicious() {
        assert!(!Nullable::NotNone.is_suspicious());
        assert!(!Nullable::Unknown.is_suspicious());
        assert!(Nullable::DefinitelyNone.is_suspicious());
        assert!(Nullable::MaybeNone.is_suspicious());
    }
}
