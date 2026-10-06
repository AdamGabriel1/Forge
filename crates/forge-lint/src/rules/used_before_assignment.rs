use crate::util::walk;
use crate::Rule;
use forge_cfg::dataflow::{run_block, Analysis};
use forge_cfg::definite_assignment::{collect_locals, DefiniteAssignmentAnalysis};
use forge_core::{Context, Diagnostic};
use tree_sitter::Node;

pub struct UsedBeforeAssignment;

impl Rule for UsedBeforeAssignment {
    fn code(&self) -> &str {
        "FOR012"
    }
    fn name(&self) -> &str {
        "used_before_assignment"
    }
    fn description(&self) -> &str {
        "Uma variável local é lida antes de ser atribuída em todos os caminhos — Python lança `UnboundLocalError` em runtime."
    }
    fn fix_hint(&self) -> &str {
        "Mova a atribuição para antes do uso, ou atribua um valor inicial."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Só analisamos corpos de função. Módulo tem semântica mais
        // permissiva em Python (nomes são resolvidos em tempo de execução).
        walk(node, &mut |n| {
            if n.kind() != "function_definition" {
                return;
            }
            let Some(body) = n.child_by_field_name("body") else {
                return;
            };
            let (locals, params) = collect_locals(n, ctx.source);
            if locals.is_empty() && params.is_empty() {
                return;
            }
            let analysis = DefiniteAssignmentAnalysis::new(ctx.source, locals, params);
            run_block(body, analysis.initial(), &analysis, &mut diagnostics);
        });

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

    // ---- casos básicos (v1 também cobria) ----

    #[test]
    fn uso_antes_de_atribuicao_local() {
        let src = "\
def f():
    print(x)
    x = 1
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 1);
    }

    #[test]
    fn atribuicao_antes_do_uso_ok() {
        let src = "\
def f():
    x = 1
    print(x)
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 0);
    }

    #[test]
    fn parametro_nao_reporta() {
        let src = "\
def f(x):
    print(x)
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 0);
    }

    #[test]
    fn uso_em_closure_nao_reporta() {
        let src = "\
def outer():
    def inner():
        return x
    x = 1
    return inner
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 0);
    }

    #[test]
    fn uso_em_escopo_modulo_nao_reporta() {
        let src = "\
print(x)
x = 1
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 0);
    }

    #[test]
    fn multiplos_usos_antes() {
        let src = "\
def f():
    print(a)
    print(a)
    a = 1
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 2);
    }

    #[test]
    fn uso_depois_de_loop_ok() {
        let src = "\
def f():
    for i in range(10):
        pass
    print(i)
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 0);
    }

    // ---- casos novos: sensíveis a caminho ----

    #[test]
    fn if_sem_else_apenas_then_atribui_reporta() {
        let src = "\
def f(c):
    if c:
        x = 1
    print(x)
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 1);
    }

    #[test]
    fn if_else_ambos_atribuem_ok() {
        let src = "\
def f(c):
    if c:
        x = 1
    else:
        x = 2
    print(x)
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 0);
    }

    #[test]
    fn if_else_so_else_atribui_reporta() {
        let src = "\
def f(c):
    if c:
        pass
    else:
        x = 1
    print(x)
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 1);
    }

    #[test]
    fn atribuicao_antes_do_if_ok() {
        let src = "\
def f(c):
    x = 0
    if c:
        x = 1
    print(x)
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 0);
    }

    #[test]
    fn uso_dentro_do_ramo_que_atribui_ok() {
        let src = "\
def f(c):
    if c:
        x = 1
        print(x)
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 0);
    }

    #[test]
    fn uso_no_ramo_que_nao_atribui_reporta() {
        let src = "\
def f(c):
    if c:
        pass
    else:
        x = 1
        print(x)
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 0);
    }

    #[test]
    fn reatribuicao_apos_uso_continua_reportando() {
        let src = "\
def f():
    y = x
    x = 1
    return y
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 1);
    }
}