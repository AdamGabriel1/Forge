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
    ///
    /// A análise pode emitir diagnósticos em `diags`.
    fn transfer<'tree>(
        &self,
        node: Node<'tree>,
        state: &Self::State,
        diags: &mut Vec<Diagnostic>,
    ) -> Self::State;

    /// Refina o estado dado um teste de condição. `positive = true` significa
    /// que estamos no ramo "condição verdadeira"; `false` significa o `else`.
    ///
    /// Default: não refina (retorna o estado original).
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
    let mut cursor = body.walk();
    let stmts: Vec<Node<'tree>> = body
        .children(&mut cursor)
        .filter(|c| c.is_named())
        .collect();

    let mut current = initial;

    for stmt in stmts {
        match stmt.kind() {
            "if_statement" => {
                let cond = stmt.child_by_field_name("condition").unwrap();
                let then_body = stmt.child_by_field_name("consequence").unwrap();
                let else_body = stmt.child_by_field_name("alternative");

                // A condição em si pode desreferenciar variáveis.
                current = analysis.transfer(cond, &current, diags);

                let s_then_in = analysis.refine(cond, &current, true);
                let s_else_in = analysis.refine(cond, &current, false);

                let mut d_then = Vec::new();
                let s_then_out = run_block(then_body, s_then_in, analysis, &mut d_then);

                let (s_else_out, d_else) = match else_body {
                    Some(eb) => {
                        let mut d = Vec::new();
                        let s = run_block(eb, s_else_in, analysis, &mut d);
                        (s, d)
                    }
                    None => (s_else_in, Vec::new()),
                };

                // Emitir os diagnósticos dos dois ramos.
                diags.extend(d_then);
                diags.extend(d_else);

                current = analysis.merge(&s_then_out, &s_else_out);
            }

            "while_statement" | "for_statement" => {
                let body_node = stmt.child_by_field_name("body").unwrap();
                let cond = stmt.child_by_field_name("condition");
                let iter = stmt.child_by_field_name("right");

                // A expressão do loop em si.
                if let Some(expr) = cond.or(iter) {
                    current = analysis.transfer(expr, &current, diags);
                }

                // Estado dentro do loop: refina pela condição (positiva).
                let loop_in = if let Some(c) = cond {
                    analysis.refine(c, &current, true)
                } else {
                    current.clone()
                };

                // Uma passada no corpo. Uma análise completa faria ponto fixo;
                // uma única passada é conservadora o suficiente para o
                // primeiro consumidor (nullable).
                let mut d_body = Vec::new();
                let body_out = run_block(body_node, loop_in.clone(), analysis, &mut d_body);
                diags.extend(d_body);

                // Depois do loop: merge entre "nunca entrou" e "saiu do corpo".
                current = analysis.merge(&current, &body_out);
            }

            "return_statement" | "raise_statement" => {
                // Processa a expressão (se houver) e encerra o bloco.
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

            _ => {
                current = analysis.transfer(stmt, &current, diags);
            }
        }
    }

    current
}
