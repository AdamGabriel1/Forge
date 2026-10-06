use crate::util::walk;
use crate::Rule;
use forge_cfg::dataflow::run_block;
use forge_cfg::nullable::NullableAnalysis;
use forge_core::{Context, Diagnostic, Severity};
use tree_sitter::Node;

pub struct PossibleNoneDereference;

impl Rule for PossibleNoneDereference {
    fn code(&self) -> &str {
        "FOR013"
    }
    fn name(&self) -> &str {
        "possible_none_dereference"
    }
    fn description(&self) -> &str {
        "Variável que pode ser `None` é desreferenciada sem checagem, causando `TypeError` em runtime."
    }
    fn fix_hint(&self) -> &str {
        "Adicione `if x is not None:` antes, ou atribua um valor não-`None`."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let analysis = NullableAnalysis::new(ctx.source);
        let mut diagnostics = Vec::new();

        // Top-level: variáveis de módulo.
        run_block(node, analysis.initial(), &analysis, &mut diagnostics);

        // Cada função tem seu próprio escopo.
        walk(node, &mut |n| {
            if n.kind() != "function_definition" {
                return;
            }
            let Some(body) = n.child_by_field_name("body") else {
                return;
            };
            run_block(body, analysis.initial(), &analysis, &mut diagnostics);
        });

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

    #[test]
    fn none_literal_imediato() {
        let src = "\
def f():
    x = None
    x.foo()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 1);
    }

    #[test]
    fn subscript_apos_none() {
        let src = "\
def f():
    x = None
    x[0]
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 1);
    }

    #[test]
    fn reatribuicao_antes_do_uso_ok() {
        let src = "\
def f():
    x = None
    x = 5
    x.foo()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 0);
    }

    #[test]
    fn sem_none_ok() {
        let src = "\
def f():
    x = 1
    x.foo()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 0);
    }

    #[test]
    fn sem_deref_ok() {
        let src = "\
def f():
    x = None
    print(x)
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 0);
    }

    #[test]
    fn modulo_top_level() {
        let src = "\
x = None
x.foo()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 1);
    }

    // ---- casos que exigem data-flow ----

    #[test]
    fn if_else_ambos_none_reporta() {
        let src = "\
def f(c):
    x = None
    if c:
        x = None
    else:
        x = None
    x.foo()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 1);
    }

    #[test]
    fn if_else_mistura_reporta() {
        // Uma branch define None, a outra não → MaybeNone.
        let src = "\
def f(c):
    x = 5
    if c:
        x = None
    x.foo()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 1);
    }

    #[test]
    fn if_not_none_guarda() {
        // O `if x is not None:` refina o estado para NotNone dentro do ramo.
        let src = "\
def f():
    x = None
    if x is not None:
        x.foo()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 0);
    }

    #[test]
    fn if_not_none_com_else_reporta_no_else() {
        let src = "\
def f():
    x = None
    if x is not None:
        x.foo()
    else:
        x.bar()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 1);
    }

    #[test]
    fn if_is_none_reporta_no_then() {
        let src = "\
def f():
    x = None
    if x is None:
        x.foo()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 1);
    }

    #[test]
    fn reatribuicao_em_ambas_as_branches_ok() {
        let src = "\
def f(c):
    x = None
    if c:
        x = 1
    else:
        x = 2
    x.foo()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 0);
    }

    #[test]
    fn modulo_scope_com_branch() {
        let src = "\
x = None
if cond:
    x = 5
x.foo()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 1);
    }

    #[test]
    fn uso_antes_da_atribuicao_nao_flagra() {
        let src = "\
def f():
    x.foo()
    x = None
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 0);
    }

    #[test]
    fn multiplos_derefs_apos_none() {
        let src = "\
def f():
    x = None
    x.foo
    x.bar
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 2);
    }
}