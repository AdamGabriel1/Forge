//! Framework de data-flow sobre a CST com ponto fixo para loops.
//!
//! Em vez de materializar um CFG explícito, o walker recursivo embute o
//! fluxo na própria recursão:
//!
//! - `if/else`: dois ramos independentes que fazem merge no ponto de
//!   junção, com `refine` aplicado à condição em cada lado.
//! - `while`/`for`: iteração até ponto fixo. O estado no loop head é
//!   recomputado até convergir (`new_head == loop_head`), com um teto
//!   de iterações como garantia de terminação.
//!
//! Esse modelo cobre as análises sensíveis a caminho implementadas
//! (nullable, definite assignment) e é a base para futuras migrações
//! para CFG materializado — o trait `Analysis` sobrevive a essa troca.

use forge_core::Diagnostic;
use tree_sitter::Node;

/// Teto de iterações do ponto fixo em loops. Para as duas análises
/// atuais (nullable e definite assignment), convergência ocorre em
/// poucas iterações — o teto protege contra análises futuras cujo
/// lattice não seja finito.
const MAX_FIXPOINT_ITERS: usize = 64;

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

    /// Aplica o efeito de "esta variável é atribuída a cada iteração do
    /// loop". Chamado uma vez, antes do ponto fixo, com o alvo do `for`.
    ///
    /// Default: no-op. Análises que rastreiam variáveis devem sobrescrever.
    fn bind_loop_target<'tree>(&self, _target: Node<'tree>, state: &Self::State) -> Self::State {
        state.clone()
    }

    /// Observa uma condição de `if`/`while` com o estado vigente.
    /// Usado por análises que emitem diagnósticos sobre a própria
    /// condição (ex: `ConstantPropagation` → FOR015).
    ///
    /// Default: no-op.
    fn observe_condition<'tree>(
        &self,
        _cond: Node<'tree>,
        _state: &Self::State,
        _diags: &mut Vec<Diagnostic>,
    ) {
    }
}

/// Roda a análise sobre um bloco (corpo de função ou módulo).
pub fn run_block<'tree, A: Analysis>(
    body: Node<'tree>,
    initial: A::State,
    analysis: &A,
    diags: &mut Vec<Diagnostic>,
) -> A::State {
    run_statements(body, initial, analysis, diags)
}

fn run_statements<'tree, A: Analysis>(
    body: Node<'tree>,
    initial: A::State,
    analysis: &A,
    diags: &mut Vec<Diagnostic>,
) -> A::State {
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
                current = run_if(stmt, current, analysis, diags);
            }
            "while_statement" | "for_statement" => {
                current = run_loop(stmt, current, analysis, diags);
            }
            "return_statement" | "raise_statement" => {
                let mut c = stmt.walk();
                for child in stmt.children(&mut c) {
                    if child.is_named() {
                        analysis.transfer(child, &current, diags);
                    }
                }
                return current;
            }
            "break_statement" | "continue_statement" => {
                return current;
            }
            // Blocos aninhados (else_clause, finally_clause, with, try).
            "block" | "else_clause" | "finally_clause" | "except_clause" | "with_statement"
            | "try_statement" => {
                current = run_statements(stmt, current, analysis, diags);
            }
            _ => {
                current = analysis.transfer(stmt, &current, diags);
            }
        }
    }

    current
}

fn run_if<'tree, A: Analysis>(
    stmt: Node<'tree>,
    state: A::State,
    analysis: &A,
    diags: &mut Vec<Diagnostic>,
) -> A::State {
    let Some(cond) = stmt.child_by_field_name("condition") else {
        return state;
    };
    let Some(cons) = stmt.child_by_field_name("consequence") else {
        return state;
    };
    let alt = stmt.child_by_field_name("alternative");

    analysis.observe_condition(cond, &state, diags);

    // A condição em si pode desreferenciar variáveis.
    let state_after_cond = analysis.transfer(cond, &state, diags);

    let s_then_in = analysis.refine(cond, &state_after_cond, true);
    let s_else_in = analysis.refine(cond, &state_after_cond, false);

    let mut d_then = Vec::new();
    let s_then_out = run_statements(cons, s_then_in, analysis, &mut d_then);

    let (s_else_out, d_else) = match alt {
        Some(eb) => {
            let mut d = Vec::new();
            let s = run_statements(eb, s_else_in, analysis, &mut d);
            (s, d)
        }
        None => (s_else_in, Vec::new()),
    };

    diags.extend(d_then);
    diags.extend(d_else);

    analysis.merge(&s_then_out, &s_else_out)
}

fn run_loop<'tree, A: Analysis>(
    stmt: Node<'tree>,
    state_before: A::State,
    analysis: &A,
    diags: &mut Vec<Diagnostic>,
) -> A::State {
    let cond = stmt.child_by_field_name("condition");
    let iter = stmt.child_by_field_name("right");
    let left = stmt.child_by_field_name("left");
    let Some(body) = stmt.child_by_field_name("body") else {
        return state_before;
    };

    if let Some(c) = cond {
        analysis.observe_condition(c, &state_before, diags);
    }

    // Processa a expressão do loop: condição (`while`) ou iterável (`for`).
    let state_after_expr = if let Some(expr) = cond.or(iter) {
        analysis.transfer(expr, &state_before, diags)
    } else {
        state_before.clone()
    };

    // O alvo do `for` é atribuído a cada iteração, logo antes do corpo.
    // Mantemos `state_after_expr` separado — ele é o estado se o loop
    // nunca executar (importante para o merge de saída).
    let state_at_head = if let Some(target) = left {
        analysis.bind_loop_target(target, &state_after_expr)
    } else {
        state_after_expr.clone()
    };

    // Ponto fixo: itera o corpo até o estado no loop head estabilizar.
    let mut loop_head = state_at_head.clone();
    let mut final_diags: Vec<Diagnostic> = Vec::new();

    for _ in 0..MAX_FIXPOINT_ITERS {
        let body_in = match cond {
            Some(c) => analysis.refine(c, &loop_head, true),
            None => loop_head.clone(),
        };

        let mut iter_diags = Vec::new();
        let body_out = run_statements(body, body_in, analysis, &mut iter_diags);

        let new_head = analysis.merge(&state_at_head, &body_out);
        final_diags = iter_diags;

        if new_head == loop_head {
            break;
        }
        loop_head = new_head;
    }

    diags.extend(final_diags);

    // Estado de saída:
    //   - Se o loop nunca executou: `state_after_expr`.
    //   - Se executou e terminou: `loop_head` refinado por `!cond`
    //     (para `for`, não há cond, então é `loop_head` direto).
    let exit_from_loop = match cond {
        Some(c) => analysis.refine(c, &loop_head, false),
        None => loop_head,
    };
    analysis.merge(&state_after_expr, &exit_from_loop)
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
