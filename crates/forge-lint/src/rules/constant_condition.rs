use crate::util::walk;
use crate::Context;
use crate::Rule;
use forge_cfg::{run_block, Analysis, ConstantPropagation};
use forge_core::Diagnostic;
use tree_sitter::Node;

pub struct ConstantCondition;

impl Rule for ConstantCondition {
    fn code(&self) -> &str {
        "FOR015"
    }
    fn name(&self) -> &str {
        "constant_condition"
    }
    fn description(&self) -> &str {
        "Uma condição de `if`/`while` sempre avalia para o mesmo valor — o outro ramo nunca executa."
    }
    fn fix_hint(&self) -> &str {
        "Remova a condição ou o ramo morto. Se é intencional (feature flag), considere comentário explicativo ou `# noqa: FOR015`."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let analysis = ConstantPropagation::new(ctx.source);
        let mut diagnostics = Vec::new();

        // Top-level: módulo.
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
    fn if_true_reporta() {
        let src = "\
def f():
    if True:
        pass
";
        assert_eq!(lint(&ConstantCondition, src).len(), 1);
    }

    #[test]
    fn if_false_reporta() {
        let src = "\
def f():
    if False:
        pass
";
        assert_eq!(lint(&ConstantCondition, src).len(), 1);
    }

    #[test]
    fn if_var_const_reporta() {
        let src = "\
def f():
    DEBUG = False
    if DEBUG:
        print('x')
";
        assert_eq!(lint(&ConstantCondition, src).len(), 1);
    }

    #[test]
    fn if_var_reatribuida_nao_reporta() {
        let src = "\
def f():
    DEBUG = False
    DEBUG = True
    if DEBUG:
        pass
";
        assert_eq!(lint(&ConstantCondition, src).len(), 0);
    }

    #[test]
    fn if_else_branches_const_diferentes_nao_reporta() {
        let src = "\
def f(c):
    x = 1
    if c:
        x = 2
    else:
        x = 3
    if x:
        pass
";
        assert_eq!(lint(&ConstantCondition, src).len(), 0);
    }

    #[test]
    fn while_false_reporta() {
        let src = "\
def f():
    while False:
        pass
";
        assert_eq!(lint(&ConstantCondition, src).len(), 1);
    }

    #[test]
    fn while_true_nao_reporta() {
        // Loop infinito clássico.
        let src = "\
def f():
    while True:
        pass
";
        assert_eq!(lint(&ConstantCondition, src).len(), 0);
    }

    #[test]
    fn comparacao_constante_reporta() {
        let src = "\
def f():
    if 1 + 1 == 3:
        pass
";
        assert_eq!(lint(&ConstantCondition, src).len(), 1);
    }

    #[test]
    fn condicao_variavel_nao_reporta() {
        let src = "\
def f(x):
    if x:
        pass
";
        assert_eq!(lint(&ConstantCondition, src).len(), 0);
    }

    #[test]
    fn not_true_reporta() {
        let src = "\
def f():
    if not True:
        pass
";
        assert_eq!(lint(&ConstantCondition, src).len(), 1);
    }
}
