use crate::util::walk;
use crate::Rule;
use forge_cfg::find_unreachable_in_block;
use forge_core::{Context, Diagnostic, Severity};
use tree_sitter::Node;

pub struct UnreachableCode;

impl Rule for UnreachableCode {
    fn code(&self) -> &str {
        "FOR011"
    }
    fn name(&self) -> &str {
        "unreachable_code"
    }
    fn description(&self) -> &str {
        "Código depois de `return`, `raise`, `break` ou `continue` nunca executa."
    }
    fn fix_hint(&self) -> &str {
        "Remova o código morto, ou mova-o para antes do terminador de fluxo."
    }

    fn check(&self, node: Node, _ctx: &Context) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        walk(node, &mut |n| {
            if n.kind() != "block" {
                return;
            }
            for range in find_unreachable_in_block(n) {
                diagnostics.push(Diagnostic::new(
                    "FOR011",
                    "Código inalcançável após terminador de fluxo.",
                    range,
                    Severity::Warning,
                ));
            }
        });
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

    #[test]
    fn codigo_apos_return() {
        let src = "\
def f():
    return 1
    x = 2
";
        assert_eq!(lint(&UnreachableCode, src).len(), 1);
    }

    #[test]
    fn return_e_ultimo_ok() {
        let src = "\
def f():
    x = 2
    return x
";
        assert_eq!(lint(&UnreachableCode, src).len(), 0);
    }

    #[test]
    fn codigo_apos_raise() {
        let src = "\
def f():
    raise ValueError('x')
    return 1
";
        assert_eq!(lint(&UnreachableCode, src).len(), 1);
    }

    #[test]
    fn break_dentro_de_loop() {
        let src = "\
for i in range(10):
    break
    print(i)
";
        assert_eq!(lint(&UnreachableCode, src).len(), 1);
    }

    #[test]
    fn continue_dentro_de_loop() {
        let src = "\
for i in range(10):
    continue
    print(i)
";
        assert_eq!(lint(&UnreachableCode, src).len(), 1);
    }

    #[test]
    fn break_no_if_nao_afeta_bloco_externo() {
        let src = "\
for i in range(10):
    if i == 5:
        break
    print(i)
";
        assert_eq!(lint(&UnreachableCode, src).len(), 0);
    }

    #[test]
    fn multiplas_linhas_apos_return() {
        let src = "\
def f():
    return 1
    x = 2
    y = 3
";
        assert_eq!(lint(&UnreachableCode, src).len(), 2);
    }

    #[test]
    fn codigo_apos_return_em_funcao_aninhada() {
        let src = "\
def outer():
    def inner():
        return 1
        x = 2
    return inner
";
        assert_eq!(lint(&UnreachableCode, src).len(), 1);
    }
}