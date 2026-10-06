//! Propagação de constantes.
//!
//! Lattice:
//!
//! ```text
//!       NotConstant  (⊤)
//!       /    |    \
//!   Int(5) Str("a") Bool(true) ...   (constantes, incomparáveis entre si)
//! ```
//!
//! Não há "bottom": a ausência de uma variável no mapa significa
//! "NotConstant" — não temos informação. Ao encontrar `x = <literal>`,
//! o valor vira uma constante concreta. No merge, constantes diferentes
//! degradam para `NotConstant`.
//!
//! O `observe_condition` avalia a condição de `if`/`while` e reporta
//! `FOR015` se ela for sempre `True`/`False`. Não reporta `while True:`
//! (padrão legítimo de loop infinito).

use crate::dataflow::Analysis;
use crate::range_of;
use forge_core::{Diagnostic, Severity};
use std::collections::HashMap;
use tree_sitter::Node;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstValue {
    NotConstant,
    Int(i64),
    Str(String),
    Bool(bool),
    None_,
}

impl ConstValue {
    fn join(self, other: Self) -> Self {
        if self == other {
            self
        } else {
            Self::NotConstant
        }
    }

    /// Retorna o valor-verdade se conhecido. `None` para `NotConstant`.
    fn truthy(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            Self::Int(i) => Some(*i != 0),
            Self::Str(s) => Some(!s.is_empty()),
            Self::None_ => Some(false),
            Self::NotConstant => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConstState {
    map: HashMap<String, ConstValue>,
}

impl ConstState {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn get(&self, name: &str) -> ConstValue {
        self.map
            .get(name)
            .cloned()
            .unwrap_or(ConstValue::NotConstant)
    }

    pub fn set(&mut self, name: &str, value: ConstValue) {
        self.map.insert(name.to_string(), value);
    }
}

pub struct ConstantPropagation<'src> {
    pub source: &'src str,
}

impl<'src> ConstantPropagation<'src> {
    pub fn new(source: &'src str) -> Self {
        Self { source }
    }

    fn src(&self) -> &[u8] {
        self.source.as_bytes()
    }

    fn eval_expr(&self, node: Node, state: &ConstState) -> ConstValue {
        let bytes = self.src();
        match node.kind() {
            "integer" => {
                if let Ok(text) = node.utf8_text(bytes) {
                    let cleaned: String = text.chars().filter(|c| *c != '_').collect();
                    if let Ok(i) = cleaned.parse::<i64>() {
                        return ConstValue::Int(i);
                    }
                }
                ConstValue::NotConstant
            }
            "true" => ConstValue::Bool(true),
            "false" => ConstValue::Bool(false),
            "none" => ConstValue::None_,
            "string" => {
                if let Ok(text) = node.utf8_text(bytes) {
                    // Ignora prefixos comuns (r, b, u, f) e tira as quotes.
                    let stripped = text
                        .trim_start_matches(|c: char| c == 'r' || c == 'b' || c == 'u' || c == 'f');
                    if stripped.len() >= 2 {
                        let inner = &stripped[1..stripped.len() - 1];
                        return ConstValue::Str(inner.to_string());
                    }
                }
                ConstValue::NotConstant
            }
            "identifier" => {
                if let Ok(name) = node.utf8_text(bytes) {
                    return state.get(name);
                }
                ConstValue::NotConstant
            }
            "parenthesized_expression" => {
                if let Some(inner) = node.named_child(0) {
                    return self.eval_expr(inner, state);
                }
                ConstValue::NotConstant
            }
            "binary_operator" => {
                let Some(left) = node.child_by_field_name("left") else {
                    return ConstValue::NotConstant;
                };
                let Some(right) = node.child_by_field_name("right") else {
                    return ConstValue::NotConstant;
                };
                let op = node
                    .children(&mut node.walk())
                    .find(|c| !c.is_named())
                    .map(|c| c.kind())
                    .unwrap_or("");
                let lv = self.eval_expr(left, state);
                let rv = self.eval_expr(right, state);
                self.eval_binary(lv, op, rv)
            }
            "comparison_operator" => {
                let mut cursor = node.walk();
                let children: Vec<Node> = node.children(&mut cursor).collect();
                // Só tratamos o caso simples: [left, op, right].
                if children.len() == 3 {
                    let lv = self.eval_expr(children[0], state);
                    let op = children[1].kind();
                    let rv = self.eval_expr(children[2], state);
                    return self.eval_compare(lv, op, rv);
                }
                ConstValue::NotConstant
            }
            "boolean_operator" => {
                let Some(left) = node.child_by_field_name("left") else {
                    return ConstValue::NotConstant;
                };
                let Some(right) = node.child_by_field_name("right") else {
                    return ConstValue::NotConstant;
                };
                let op = node
                    .child_by_field_name("operator")
                    .map(|n| n.kind().to_string())
                    .unwrap_or_default();
                let lv = self.eval_expr(left, state);
                let rv = self.eval_expr(right, state);
                match op.as_str() {
                    "and" => {
                        let l = lv.truthy();
                        if l == Some(false) {
                            return ConstValue::Bool(false);
                        }
                        if l == Some(true) {
                            return rv
                                .truthy()
                                .map(ConstValue::Bool)
                                .unwrap_or(ConstValue::NotConstant);
                        }
                        ConstValue::NotConstant
                    }
                    "or" => {
                        let l = lv.truthy();
                        if l == Some(true) {
                            return ConstValue::Bool(true);
                        }
                        if l == Some(false) {
                            return rv
                                .truthy()
                                .map(ConstValue::Bool)
                                .unwrap_or(ConstValue::NotConstant);
                        }
                        ConstValue::NotConstant
                    }
                    _ => ConstValue::NotConstant,
                }
            }
            "not_operator" => {
                if let Some(arg) = node.named_child(0) {
                    let v = self.eval_expr(arg, state);
                    return v
                        .truthy()
                        .map(|b| ConstValue::Bool(!b))
                        .unwrap_or(ConstValue::NotConstant);
                }
                ConstValue::NotConstant
            }
            _ => ConstValue::NotConstant,
        }
    }

    fn eval_binary(&self, l: ConstValue, op: &str, r: ConstValue) -> ConstValue {
        if l == ConstValue::NotConstant || r == ConstValue::NotConstant {
            return ConstValue::NotConstant;
        }
        match (l, r) {
            (ConstValue::Int(a), ConstValue::Int(b)) => match op {
                "+" => a
                    .checked_add(b)
                    .map(ConstValue::Int)
                    .unwrap_or(ConstValue::NotConstant),
                "-" => a
                    .checked_sub(b)
                    .map(ConstValue::Int)
                    .unwrap_or(ConstValue::NotConstant),
                "*" => a
                    .checked_mul(b)
                    .map(ConstValue::Int)
                    .unwrap_or(ConstValue::NotConstant),
                "//" => {
                    if b == 0 {
                        ConstValue::NotConstant
                    } else {
                        ConstValue::Int(a.div_euclid(b))
                    }
                }
                "%" => {
                    if b == 0 {
                        ConstValue::NotConstant
                    } else {
                        ConstValue::Int(a.rem_euclid(b))
                    }
                }
                _ => ConstValue::NotConstant,
            },
            (ConstValue::Str(a), ConstValue::Str(b)) if op == "+" => ConstValue::Str(a + &b),
            _ => ConstValue::NotConstant,
        }
    }

    fn eval_compare(&self, l: ConstValue, op: &str, r: ConstValue) -> ConstValue {
        if l == ConstValue::NotConstant || r == ConstValue::NotConstant {
            return ConstValue::NotConstant;
        }
        let eq = l == r;
        match op {
            "==" => ConstValue::Bool(eq),
            "!=" => ConstValue::Bool(!eq),
            "<" | "<=" | ">" | ">=" => match (&l, &r) {
                (ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Bool(match op {
                    "<" => a < b,
                    "<=" => a <= b,
                    ">" => a > b,
                    ">=" => a >= b,
                    _ => unreachable!(),
                }),
                (ConstValue::Str(a), ConstValue::Str(b)) => ConstValue::Bool(match op {
                    "<" => a < b,
                    "<=" => a <= b,
                    ">" => a > b,
                    ">=" => a >= b,
                    _ => unreachable!(),
                }),
                _ => ConstValue::NotConstant,
            },
            _ => ConstValue::NotConstant,
        }
    }
}

impl<'src> Analysis for ConstantPropagation<'src> {
    type State = ConstState;

    fn initial(&self) -> Self::State {
        ConstState::new()
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
            "expression_statement" => {
                if let Some(inner) = node.named_child(0) {
                    return self.transfer(inner, state, diags);
                }
            }
            "assignment" => {
                if let Some(right) = node.child_by_field_name("right") {
                    let value = self.eval_expr(right, state);
                    if let Some(left) = node.child_by_field_name("left") {
                        if left.kind() == "identifier" {
                            if let Ok(name) = left.utf8_text(bytes) {
                                new.set(name, value);
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        new
    }

    fn merge(&self, a: &Self::State, b: &Self::State) -> Self::State {
        let mut new = ConstState::new();
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

    fn observe_condition<'tree>(
        &self,
        cond: Node<'tree>,
        state: &Self::State,
        diags: &mut Vec<Diagnostic>,
    ) {
        let Some(b) = self.eval_expr(cond, state).truthy() else {
            return;
        };

        let is_while = cond
            .parent()
            .map(|p| p.kind() == "while_statement")
            .unwrap_or(false);

        // `while True:` é padrão legítimo de loop infinito.
        if is_while && b {
            return;
        }

        let msg = if b {
            "Condição é sempre `True` — o ramo `else` (se houver) nunca executa."
        } else {
            "Condição é sempre `False` — o corpo do bloco nunca executa."
        };
        diags.push(Diagnostic::new(
            "FOR015",
            msg,
            range_of(cond),
            Severity::Warning,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_constantes_iguais() {
        assert_eq!(
            ConstValue::Int(5).join(ConstValue::Int(5)),
            ConstValue::Int(5)
        );
    }

    #[test]
    fn join_constantes_diferentes() {
        assert_eq!(
            ConstValue::Int(5).join(ConstValue::Int(3)),
            ConstValue::NotConstant
        );
        assert_eq!(
            ConstValue::Bool(true).join(ConstValue::Bool(false)),
            ConstValue::NotConstant
        );
    }

    #[test]
    fn join_com_not_constant() {
        assert_eq!(
            ConstValue::Int(5).join(ConstValue::NotConstant),
            ConstValue::NotConstant
        );
    }

    #[test]
    fn truthy() {
        assert_eq!(ConstValue::Bool(true).truthy(), Some(true));
        assert_eq!(ConstValue::Int(0).truthy(), Some(false));
        assert_eq!(ConstValue::Int(1).truthy(), Some(true));
        assert_eq!(ConstValue::Str("".into()).truthy(), Some(false));
        assert_eq!(ConstValue::Str("a".into()).truthy(), Some(true));
        assert_eq!(ConstValue::None_.truthy(), Some(false));
        assert_eq!(ConstValue::NotConstant.truthy(), None);
    }
}
