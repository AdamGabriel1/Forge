use crate::Rule;
use forge_core::{Context, Diagnostic, Severity};
use forge_semantic::{BindingKind, ScopeKind, SemanticModel};
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
        "Uma variável local é lida antes de qualquer atribuição — Python lança `UnboundLocalError`."
    }
    fn fix_hint(&self) -> &str {
        "Mova a atribuição para antes do uso, ou atribua um valor inicial (`None`)."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let model = SemanticModel::analyze(node, ctx.source);
        let mut diagnostics = Vec::new();

        for (name, use_range, use_scope, resolved_scope) in model.resolved_uses() {
            // Só olhamos usos que resolvem no mesmo escopo onde ocorrem.
            // Se resolvem em escopo ancestral (closure), Python captura
            // de forma lazy — não é bug.
            if use_scope != resolved_scope {
                continue;
            }

            let scope = &model.scopes[resolved_scope];
            // Só em escopos de função.
            if scope.kind != ScopeKind::Function {
                continue;
            }

            let Some(binding) = scope.bindings.get(name) else {
                continue;
            };

            // Parâmetros existem desde o início da função.
            if matches!(binding.kind, BindingKind::Parameter) {
                continue;
            }

            // Comparamos posições lexicais. Se o uso está depois da
            // primeira atribuição no arquivo, assumimos OK — não
            // detectamos "atribuição só em um branch do if".
            let use_pos = (use_range.start_line, use_range.start_col);
            let bind_pos = (binding.range.start_line, binding.range.start_col);
            if use_pos < bind_pos {
                diagnostics.push(Diagnostic::new(
                    "FOR012",
                    &format!(
                        "`{}` é usado antes de ser atribuído — `UnboundLocalError` em tempo de execução.",
                        name
                    ),
                    use_range.clone(),
                    Severity::Warning,
                ));
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

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
        // Python captura `x` de forma lazy — `inner` só vê `x`
        // quando é chamado, e nessa altura `x` já existe.
        let src = "\
def outer():
    def inner():
        return x
    x = 1
    return inner()
";
        assert_eq!(lint(&UsedBeforeAssignment, src).len(), 0);
    }

    #[test]
    fn uso_em_escopo_modulo_nao_reporta() {
        // Módulo é top-level; ordem é a do arquivo, mas Python
        // resolve nomes em tempo de execução — não é "assignment".
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
}
