use crate::Context;
use crate::Rule;
use forge_core::{Diagnostic, Severity};
use tree_sitter::Node;

pub struct UndefinedName;

impl Rule for UndefinedName {
    fn code(&self) -> &str {
        "FOR010"
    }
    fn name(&self) -> &str {
        "undefined_name"
    }
    fn description(&self) -> &str {
        "Nome usado mas nunca definido neste arquivo (não é builtin, import ou variável local)."
    }
    fn fix_hint(&self) -> &str {
        "Verifique se há um import faltando, um typo, ou uma variável que esqueceu de definir."
    }

    fn check(&self, _node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let model = ctx.semantic();
        let mut diagnostics = Vec::new();

        for (name, range) in model.unresolved_uses() {
            if is_ignored(name) {
                continue;
            }
            diagnostics.push(Diagnostic::new(
                "FOR010",
                &format!("Nome `{}` não está definido.", name),
                range.clone(),
                Severity::Warning,
            ));
        }

        diagnostics
    }
}

fn is_ignored(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    if crate::rules::shadowed_builtin::is_builtin(name) {
        return true;
    }
    if name.starts_with("__") && name.ends_with("__") {
        return true;
    }
    if name == "self" || name == "cls" {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

    #[test]
    fn nome_simples_indefinido() {
        assert_eq!(lint(&UndefinedName, "print(x)\n").len(), 1);
    }

    #[test]
    fn nome_definido_acima() {
        let src = "x = 1\nprint(x)\n";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn nome_definido_depois_conta() {
        let src = "print(x)\nx = 1\n";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn builtin_nao_reporta() {
        let src = "print(len([1, 2, 3]))\n";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn dunder_nao_reporta() {
        let src = "print(__name__)\n";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn self_nao_reporta() {
        let src = "\
class A:
    def m(self):
        return self
";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn import_simples_resolve() {
        let src = "import os\nprint(os.getcwd())\n";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn import_aliased_resolve() {
        let src = "import os as o\nprint(o.getcwd())\n";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn from_import_resolve() {
        let src = "from os import path\nprint(path)\n";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn wildcard_import_suprime() {
        let src = "from os import *\nprint(getcwd())\n";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn parametro_resolve() {
        let src = "def f(x):\n    return x\n";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn local_resolve() {
        let src = "def f():\n    y = 1\n    return y\n";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn closure_resolve() {
        let src = "\
def outer():
    x = 1
    def inner():
        return x
";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn for_target_resolve() {
        let src = "for i in range(10):\n    print(i)\n";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn with_target_resolve() {
        let src = "with open('f') as fh:\n    print(fh)\n";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn except_target_resolve() {
        let src = "try:\n    pass\nexcept Exception as e:\n    print(e)\n";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn nome_de_funcao_resolve_apos_definicao() {
        let src = "\
def f():
    return 1

print(f())
";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn chama_funcao_antes_de_definir() {
        let src = "\
print(f())

def f():
    return 1
";
        assert_eq!(lint(&UndefinedName, src).len(), 0);
    }

    #[test]
    fn multiplos_indefinidos() {
        let src = "print(a, b, c)\n";
        assert_eq!(lint(&UndefinedName, src).len(), 3);
    }

    #[test]
    fn atributo_indefinido_nao_reporta() {
        let src = "obj.metodo()\n";
        assert_eq!(lint(&UndefinedName, src).len(), 1);
    }
}
