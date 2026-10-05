use crate::util::{range_of, walk};
use crate::Rule;
use forge_core::{Context, Diagnostic, Severity};
use std::collections::HashMap;
use tree_sitter::Node;

pub struct RedefinedFunction;

impl Rule for RedefinedFunction {
    fn code(&self) -> &str {
        "FOR009"
    }
    fn name(&self) -> &str {
        "redefined_function"
    }
    fn description(&self) -> &str {
        "Duas funções/classes com o mesmo nome no mesmo escopo: a segunda sobrescreve a primeira."
    }
    fn fix_hint(&self) -> &str {
        "Renomeie uma das definições ou remova a duplicata."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        walk(node, &mut |n| {
            // Apenas nós que contêm corpo de escopo: module, block.
            if !matches!(n.kind(), "module" | "block") {
                return;
            }
            check_body_for_redefinitions(n, ctx.source, &mut diagnostics);
        });
        diagnostics
    }
}

fn check_body_for_redefinitions(body: Node, source: &str, out: &mut Vec<Diagnostic>) {
    // nome -> Vec<Range> (ranges das definições que usam esse nome)
    let mut seen: HashMap<String, Vec<forge_core::Range>> = HashMap::new();

    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        if !matches!(
            child.kind(),
            "function_definition" | "class_definition"
        ) {
            continue;
        }
        let Some(name_node) = child.child_by_field_name("name") else {
            continue;
        };
        let Ok(name) = name_node.utf8_text(source.as_bytes()) else {
            continue;
        };
        seen.entry(name.to_string())
            .or_default()
            .push(range_of(child));
    }

    for (name, ranges) in seen {
        if ranges.len() > 1 {
            for r in ranges.into_iter().skip(1) {
                out.push(Diagnostic::new(
                    "FOR009",
                    &format!("`{}` é redefinida neste escopo.", name),
                    r,
                    Severity::Warning,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

    #[test]
    fn funcao_redefinida_no_modulo() {
        let src = "\
def f():
    pass

def f():
    pass
";
        assert_eq!(lint(&RedefinedFunction, src).len(), 1);
    }

    #[test]
    fn funcoes_diferentes_nao_reportam() {
        let src = "\
def f():
    pass

def g():
    pass
";
        assert_eq!(lint(&RedefinedFunction, src).len(), 0);
    }

    #[test]
    fn classe_redefinida() {
        let src = "\
class A:
    pass

class A:
    pass
";
        assert_eq!(lint(&RedefinedFunction, src).len(), 1);
    }

    #[test]
    fn funcao_e_classe_mesmo_nome() {
        let src = "\
def A():
    pass

class A:
    pass
";
        assert_eq!(lint(&RedefinedFunction, src).len(), 1);
    }

    #[test]
    fn mesma_funcao_em_escopos_diferentes() {
        let src = "\
def f():
    pass

def outer():
    def f():
        pass
    return f
";
        assert_eq!(lint(&RedefinedFunction, src).len(), 0);
    }

    #[test]
    fn tres_definicoes_reportam_duas() {
        let src = "\
def f():
    pass

def f():
    pass

def f():
    pass
";
        assert_eq!(lint(&RedefinedFunction, src).len(), 2);
    }

    #[test]
    fn dentro_de_if_nao_pega() {
        // Filhas diretas do `if` não são do mesmo bloco; a regra não cobre
        // condicionais (limitação conhecida — apenas corpos diretos).
        let src = "\
def f():
    pass

if True:
    def f():
        pass
";
        assert_eq!(lint(&RedefinedFunction, src).len(), 0);
    }
}
