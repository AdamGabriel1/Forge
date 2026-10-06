//! Framework minimalista de data-flow sobre a CST.
//!
//! Em vez de materializar um CFG explícito com nós e arestas, o walker
//! recursivo embute o fluxo na própria recursão: `if/else` vira dois
//! ramos independentes que fazem merge no ponto de junção; `while/for`
//! viram um único passo com merge conservador.
//!
//! Esse modelo é suficiente para análises sensíveis a caminho como
//! "possivelmente None". Quando quisermos `constant propagation` ou
//! `definite assignment` de verdade, migramos para um CFG materializado.

use forge_core::Diagnostic;
use tree_sitter::Node;

pub trait Analysis {
    /// Estado abstrato que flui pelos caminhos.
    type State: Clone + PartialEq;

    /// Estado no início de um escopo (função ou módulo).
    fn initial(&self) -> Self::State;

    /// Processa um único nó e retorna o estado de saída.
    fn transfer<'tree>(
        &self,
        node: Node<'tree>,
        state: &Self::State,
        diags: &mut Vec<Diagnostic>,
    ) -> Self::State;

    /// Refina o estado dado um teste de condição. `positive = true` significa
    /// que estamos no ramo "condição verdadeira"; `false` significa o `else`.
    fn refine<'tree>(
        &self,
        _cond: Node<'tree>,
        state: &Self::State,
        _positive: bool,
    ) -> Self::State {
        state.clone()
    }

    /// Combina dois estados de caminhos diferentes no ponto de junção.
    fn merge(&self, a: &Self::State, b: &Self::State) -> Self::State;
}

/// Roda a análise sobre um bloco (corpo de função ou módulo).
pub fn run_block<'tree, A: Analysis>(
    body: Node<'tree>,
    initial: A::State,
    analysis: &A,
    diags: &mut Vec<Diagnostic>,
) -> A::State {
    // Alguns wrappers vêm com o `block` dentro (else_clause, finally_clause).
    // Desembrulhamos para que os statements internos sejam vistos.
    let body = unwrap_body(body);

    let mut cursor = body.walk();
    let stmts: Vec<Node<'tree>> = body
        .children(&mut cursor)
        .filter(|c| c.is_named())
        .collect();

    let mut current = initial;

    for stmt in stmts {
        match stmt.kind() {
            "if_statement" => {
                let Some(cond) = stmt.child_by_field_name("condition") else {
                    continue;
                };
                let Some(cons) = stmt.child_by_field_name("consequence") else {
                    continue;
                };
                let alt = stmt.child_by_field_name("alternative");

                // A condição em si pode desreferenciar variáveis.
                current = analysis.transfer(cond, &current, diags);

                let s_then_in = analysis.refine(cond, &current, true);
                let s_else_in = analysis.refine(cond, &current, false);

                let mut d_then = Vec::new();
                let s_then_out = run_block(cons, s_then_in, analysis, &mut d_then);

                let (s_else_out, d_else) = match alt {
                    Some(eb) => {
                        let mut d = Vec::new();
                        let s = run_block(eb, s_else_in, analysis, &mut d);
                        (s, d)
                    }
                    None => (s_else_in, Vec::new()),
                };

                diags.extend(d_then);
                diags.extend(d_else);

                current = analysis.merge(&s_then_out, &s_else_out);
            }

            "while_statement" | "for_statement" => {
                let Some(body_node) = stmt.child_by_field_name("body") else {
                    continue;
                };
                let cond = stmt.child_by_field_name("condition");
                let iter = stmt.child_by_field_name("right");

                if let Some(expr) = cond.or(iter) {
                    current = analysis.transfer(expr, &current, diags);
                }

                let loop_in = if let Some(c) = cond {
                    analysis.refine(c, &current, true)
                } else {
                    current.clone()
                };

                let mut d_body = Vec::new();
                let body_out = run_block(body_node, loop_in.clone(), analysis, &mut d_body);
                diags.extend(d_body);

                current = analysis.merge(&current, &body_out);
            }

            "return_statement" | "raise_statement" => {
                let mut cursor = stmt.walk();
                for child in stmt.children(&mut cursor) {
                    if child.is_named() {
                        analysis.transfer(child, &current, diags);
                    }
                }
                return current;
            }

            "break_statement" | "continue_statement" => {
                return current;
            }

            // Blocos aninhados (else_clause, finally_clause wrappers,
            // try/except/with) — processa como novo bloco.
            "block" | "else_clause" | "finally_clause" | "except_clause" | "with_statement"
            | "try_statement" => {
                current = run_block(stmt, current, analysis, diags);
            }

            _ => {
                current = analysis.transfer(stmt, &current, diags);
            }
        }
    }

    current
}

/// Se `node` é um wrapper (else_clause, finally_clause, etc.) que contém
/// um `block`, retorna o `block`. Caso contrário, retorna `node`.
fn unwrap_body(node: Node) -> Node {
    if node.kind() == "block" {
        return node;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "block" {
            return child;
        }
    }
    node
}
