use crate::Rule;
use forge_core::{Context, Diagnostic, Severity};
use forge_semantic::{BindingKind, ScopeKind, SemanticModel};
use tree_sitter::Node;

pub struct UnusedVariable;

impl Rule for UnusedVariable {
    fn code(&self) -> &str {
        "FOR007"
    }
    fn name(&self) -> &str {
        "unused_variable"
    }
    fn description(&self) -> &str {
        "Variáveis locais ou de módulo que nunca são lidas deveriam ser removidas."
    }
    fn fix_hint(&self) -> &str {
        "Remova a variável, use `_` para descartar, ou prefixe com `_` se for intencional."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let model = SemanticModel::analyze(node, ctx.source);
        let mut diagnostics = Vec::new();

        for (scope, binding) in model.bindings() {
            if binding.used {
                continue;
            }
            if binding.name.starts_with('_') {
                continue;
            }
            // Não reportamos parâmetros, funções nem classes. Parâmetros são
            // frequentemente exigidos por contratos; funções/classes são
            // "usadas" pelo simples fato de existirem.
            if matches!(
                binding.kind,
                BindingKind::Parameter
                    | BindingKind::Function
                    | BindingKind::Class
                    | BindingKind::Import
            ) {
                continue;
            }
            // Escopos considerados: módulo e função.
            if !matches!(scope.kind, ScopeKind::Module | ScopeKind::Function) {
                continue;
            }

            diagnostics.push(Diagnostic::new(
                "FOR007",
                &format!("Variável `{}` nunca é usada.", binding.name),
                binding.range.clone(),
                Severity::Warning,
            ));
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

    #[test]
    fn var_simples_nao_usada() {
        assert_eq!(lint(&UnusedVariable, "x = 1\n").len(), 1);
    }

    #[test]
    fn var_simples_usada() {
        let src = "x = 1\nprint(x)\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 0);
    }

    #[test]
    fn local_nao_usada() {
        let src = "def f():\n    x = 1\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 1);
    }

    #[test]
    fn local_usada() {
        let src = "def f():\n    x = 1\n    return x\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 0);
    }

    #[test]
    fn closure_conta_como_uso() {
        let src = "\
def outer():
    x = 1
    def inner():
        return x
";
        assert_eq!(lint(&UnusedVariable, src).len(), 0);
    }

    #[test]
    fn for_target_nao_usado() {
        let src = "for i in range(10):\n    pass\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 1);
    }

    #[test]
    fn for_target_usado() {
        let src = "for i in range(10):\n    print(i)\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 0);
    }

    #[test]
    fn with_target_nao_usado() {
        let src = "with open('f') as fh:\n    pass\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 1);
    }

    #[test]
    fn with_target_usado() {
        let src = "with open('f') as fh:\n    print(fh)\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 0);
    }

    #[test]
    fn except_target_nao_usado() {
        let src = "try:\n    pass\nexcept Exception as e:\n    pass\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 1);
    }

    #[test]
    fn except_target_usado() {
        let src = "try:\n    pass\nexcept Exception as e:\n    print(e)\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 0);
    }

    #[test]
    fn underscore_e_ignorada() {
        assert_eq!(lint(&UnusedVariable, "_ = 1\n").len(), 0);
    }

    #[test]
    fn dunder_e_ignorada() {
        assert_eq!(lint(&UnusedVariable, "__version__ = '1.0'\n").len(), 0);
    }

    #[test]
    fn parametro_nao_reportado() {
        let src = "def f(x):\n    return 1\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 0);
    }

    #[test]
    fn funcao_nao_reportada() {
        let src = "def f():\n    return 1\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 0);
    }

    #[test]
    fn classe_nao_reportada() {
        let src = "class C:\n    pass\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 0);
    }

    #[test]
    fn tupla_parcialmente_usada() {
        let src = "a, b = 1, 2\nprint(a)\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 1);
    }

    #[test]
    fn tupla_totalmente_usada() {
        let src = "a, b = 1, 2\nprint(a, b)\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 0);
    }

    #[test]
    fn reassign_marca_usada() {
        let src = "\
x = 1
x = 2
print(x)
";
        assert_eq!(lint(&UnusedVariable, src).len(), 0);
    }

    #[test]
    fn walrus_usado() {
        let src = "if (n := 10) > 5:\n    print(n)\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 0);
    }

    #[test]
    fn comprehension_nao_reporta() {
        let src = "squares = [i * i for i in range(10)]\nprint(squares)\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 0);
    }

    #[test]
    fn multiplas_nao_usadas() {
        let src = "\
def f():
    a = 1
    b = 2
    return 3
";
        assert_eq!(lint(&UnusedVariable, src).len(), 2);
    }

    #[test]
    fn import_nao_reportado_como_variavel() {
        let src = "import os\n";
        assert_eq!(lint(&UnusedVariable, src).len(), 0);
    }
}
