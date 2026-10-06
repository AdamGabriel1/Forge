use crate::Context;
use crate::Rule;
use forge_core::{Diagnostic, Severity};
use forge_semantic::{BindingKind, ScopeKind};
use tree_sitter::Node;

pub struct ShadowedVariable;

impl Rule for ShadowedVariable {
    fn code(&self) -> &str {
        "FOR008"
    }
    fn name(&self) -> &str {
        "shadowed_variable"
    }
    fn description(&self) -> &str {
        "Uma variável local com o mesmo nome de uma variável externa pode causar bugs sutis."
    }
    fn fix_hint(&self) -> &str {
        "Renomeie a variável interna para deixar claro que não é a mesma do escopo externo."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let model = ctx.semantic();
        let mut diagnostics = Vec::new();

        for scope in &model.scopes {
            if !matches!(scope.kind, ScopeKind::Module | ScopeKind::Function) {
                continue;
            }

            for binding in scope.bindings.values() {
                if is_ignored(binding) {
                    continue;
                }

                let mut current = scope.parent;
                while let Some(pid) = current {
                    let parent = &model.scopes[pid];
                    if matches!(parent.kind, ScopeKind::Module | ScopeKind::Function)
                        && parent.bindings.contains_key(&binding.name)
                    {
                        diagnostics.push(Diagnostic::new(
                            "FOR008",
                            &format!(
                                "`{}` faz shadowing de uma variável do escopo externo.",
                                binding.name
                            ),
                            binding.range.clone(),
                            Severity::Warning,
                        ));
                        break;
                    }
                    current = parent.parent;
                }
            }
        }

        diagnostics
    }
}

fn is_ignored(binding: &forge_semantic::Binding) -> bool {
    if binding.name.starts_with('_') {
        return true;
    }
    if binding.name == "self" || binding.name == "cls" {
        return true;
    }
    matches!(binding.kind, BindingKind::Function | BindingKind::Class)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

    #[test]
    fn local_shadowing_modulo() {
        let src = "\
x = 1
def f():
    x = 2
    return x
";
        assert_eq!(lint(&ShadowedVariable, src).len(), 1);
    }

    #[test]
    fn parametro_shadowing_modulo() {
        let src = "\
x = 1
def f(x):
    return x
";
        assert_eq!(lint(&ShadowedVariable, src).len(), 1);
    }

    #[test]
    fn sem_shadowing() {
        let src = "\
x = 1
def f():
    y = 2
    return y
";
        assert_eq!(lint(&ShadowedVariable, src).len(), 0);
    }

    #[test]
    fn self_nao_reportado() {
        let src = "\
class A:
    def m(self):
        self = 1
";
        assert_eq!(lint(&ShadowedVariable, src).len(), 0);
    }

    #[test]
    fn underscore_nao_reportado() {
        let src = "\
_x = 1
def f():
    _x = 2
    return _x
";
        assert_eq!(lint(&ShadowedVariable, src).len(), 0);
    }

    #[test]
    fn shadowing_aninhado_profundo() {
        let src = "\
x = 1
def outer():
    def inner():
        x = 2
        return x
";
        assert_eq!(lint(&ShadowedVariable, src).len(), 1);
    }

    #[test]
    fn shadowing_entre_funcoes_irmao_nao_conta() {
        let src = "\
def a():
    x = 1
    return x

def b():
    x = 2
    return x
";
        assert_eq!(lint(&ShadowedVariable, src).len(), 0);
    }

    #[test]
    fn funcao_com_mesmo_nome_nao_reportada() {
        let src = "\
def f():
    pass

def g():
    def f():
        pass
    return f
";
        assert_eq!(lint(&ShadowedVariable, src).len(), 0);
    }
}
