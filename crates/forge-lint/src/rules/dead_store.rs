use crate::util::walk;
use crate::Context;
use crate::Rule;
use forge_cfg::{run_block, Analysis, DefId, ReachState, ReachingDefinitions};
use forge_core::{Diagnostic, Range, Severity};
use std::collections::HashMap;
use tree_sitter::Node;

pub struct DeadStore;

impl Rule for DeadStore {
    fn code(&self) -> &str {
        "FOR016"
    }
    fn name(&self) -> &str {
        "dead_store"
    }
    fn description(&self) -> &str {
        "Uma atribuição cujo valor nunca é lido — a variável é sobrescrita antes de qualquer uso."
    }
    fn fix_hint(&self) -> &str {
        "Remova a atribuição, ou use o valor antes de reatribuir."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let analysis = ReachingDefinitions::new(ctx.source);
        let mut diagnostics = Vec::new();

        // Escopo do módulo.
        let state = run_block(node, analysis.initial(), &analysis, &mut diagnostics);
        emit_dead(&state, &mut diagnostics);

        // Escopo de cada função.
        walk(node, &mut |n| {
            if n.kind() != "function_definition" {
                return;
            }
            let Some(body) = n.child_by_field_name("body") else {
                return;
            };
            let state = run_block(body, analysis.initial(), &analysis, &mut diagnostics);
            emit_dead(&state, &mut diagnostics);
        });

        diagnostics
    }
}

fn emit_dead(state: &ReachState, diags: &mut Vec<Diagnostic>) {
    // Agrupa defs por nome — o filtro é por variável, não por def.
    let mut by_name: HashMap<&str, Vec<&DefId>> = HashMap::new();
    for d in &state.all_defs {
        by_name.entry(d.name.as_str()).or_default().push(d);
    }

    for (name, defs) in by_name {
        if name.starts_with('_') {
            continue;
        }
        // Se NENHUMA def dessa var foi usada, FOR007 já cobre o caso.
        let any_used = defs.iter().any(|d| state.used.contains(*d));
        if !any_used {
            continue;
        }
        // Reporta cada def individualmente não usada.
        for d in defs {
            if state.used.contains(d) {
                continue;
            }
            diags.push(Diagnostic::new(
                "FOR016",
                &format!("Atribuição a `{}` cujo valor nunca é lido.", d.name),
                Range {
                    start_line: d.line,
                    start_col: d.col,
                    end_line: d.line,
                    end_col: d.end_col,
                },
                Severity::Warning,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

    #[test]
    fn dead_store_simples() {
        let src = "\
def f():
    x = 1
    x = 2
    print(x)
";
        assert_eq!(lint(&DeadStore, src).len(), 1);
    }

    #[test]
    fn dead_store_apos_uso() {
        let src = "\
def f():
    x = 1
    print(x)
    x = 2
";
        assert_eq!(lint(&DeadStore, src).len(), 1);
    }

    #[test]
    fn uso_simples_nao_reporta() {
        let src = "\
def f():
    x = 1
    print(x)
";
        assert_eq!(lint(&DeadStore, src).len(), 0);
    }

    #[test]
    fn branches_ambas_sobrescrevem() {
        let src = "\
def f(c):
    x = 1
    if c:
        x = 2
    else:
        x = 3
    print(x)
";
        assert_eq!(lint(&DeadStore, src).len(), 1);
    }

    #[test]
    fn apenas_um_branch_sobrescreve_nao_reporta() {
        let src = "\
def f(c):
    x = 1
    if c:
        x = 2
    print(x)
";
        assert_eq!(lint(&DeadStore, src).len(), 0);
    }

    #[test]
    fn variavel_nunca_usada_nao_duplica() {
        // Coberto por FOR007. FOR016 não reporta.
        let src = "\
def f():
    x = 1
    x = 2
";
        assert_eq!(lint(&DeadStore, src).len(), 0);
    }

    #[test]
    fn underscore_ignorado() {
        let src = "\
def f():
    _ = compute()
    _ = other()
    print(_)
";
        assert_eq!(lint(&DeadStore, src).len(), 0);
    }

    #[test]
    fn augmented_assignment_le_antes() {
        let src = "\
def f():
    x = 1
    x += 1
    print(x)
";
        assert_eq!(lint(&DeadStore, src).len(), 0);
    }

    #[test]
    fn loop_var_nao_reporta() {
        let src = "\
def f(items):
    for i in items:
        print(i)
";
        assert_eq!(lint(&DeadStore, src).len(), 0);
    }

    #[test]
    fn dead_store_em_loop() {
        let src = "\
def f(items):
    for i in items:
        x = i
        print(x)
        x = i + 1
";
        assert_eq!(lint(&DeadStore, src).len(), 1);
    }

    #[test]
    fn atribuicao_no_then_e_leitura_depois_ok() {
        let src = "\
def f(c):
    x = 1
    if c:
        x = 2
    print(x)
";
        assert_eq!(lint(&DeadStore, src).len(), 0);
    }

    #[test]
    fn modulo_scope() {
        let src = "\
x = 1
x = 2
print(x)
";
        assert_eq!(lint(&DeadStore, src).len(), 1);
    }
}
