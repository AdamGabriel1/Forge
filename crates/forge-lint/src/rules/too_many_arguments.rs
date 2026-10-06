use crate::util::range_of;
use crate::Context;
use crate::Rule;
use forge_core::{Diagnostic, Severity};
use tree_sitter::Node;

const DEFAULT_MAX_ARGS: usize = 5;

pub struct TooManyArguments;

impl Rule for TooManyArguments {
    fn code(&self) -> &str {
        "FOR003"
    }
    fn name(&self) -> &str {
        "too_many_arguments"
    }
    fn description(&self) -> &str {
        "Funções com muitos parâmetros são difíceis de entender, testar e evoluir."
    }
    fn fix_hint(&self) -> &str {
        "Agrupe parâmetros relacionados em um dataclass/objeto, ou divida a função."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let max_args = ctx
            .config
            .lint
            .option_usize("FOR003", "max-args")
            .unwrap_or(DEFAULT_MAX_ARGS);

        let mut diagnostics = Vec::new();
        crate::util::walk(node, &mut |n| {
            if n.kind() != "function_definition" {
                return;
            }
            let count = count_parameters(n, ctx.source);
            if count > max_args {
                let name = n
                    .child_by_field_name("name")
                    .and_then(|id| id.utf8_text(ctx.source.as_bytes()).ok())
                    .unwrap_or("<função>");
                diagnostics.push(Diagnostic::new(
                    "FOR003",
                    &format!(
                        "Função `{}` tem {} parâmetros (máximo: {}).",
                        name, count, max_args
                    ),
                    range_of(n),
                    Severity::Warning,
                ));
            }
        });
        diagnostics
    }
}

fn count_parameters(func_def: Node, source: &str) -> usize {
    let Some(params) = func_def.child_by_field_name("parameters") else {
        return 0;
    };

    let mut cursor = params.walk();
    let params_vec: Vec<Node> = params
        .children(&mut cursor)
        .filter(|c| {
            matches!(
                c.kind(),
                "identifier"
                    | "default_parameter"
                    | "typed_parameter"
                    | "typed_default_parameter"
                    | "list_splat_pattern"
                    | "dictionary_splat_pattern"
            )
        })
        .collect();

    if is_method(func_def) {
        if let Some(first) = params_vec.first() {
            if first.kind() == "identifier" {
                if let Ok(text) = first.utf8_text(source.as_bytes()) {
                    if text == "self" || text == "cls" {
                        return params_vec.len().saturating_sub(1);
                    }
                }
            }
        }
    }

    params_vec.len()
}

fn is_method(func_def: Node) -> bool {
    let mut current = func_def.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "class_definition" => return true,
            "function_definition" | "module" => return false,
            _ => current = parent.parent(),
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::{config_with_option, lint, lint_with};

    #[test]
    fn aceita_ate_5() {
        let src = "def f(a, b, c, d, e):\n    pass\n";
        assert_eq!(lint(&TooManyArguments, src).len(), 0);
    }

    #[test]
    fn flag_6() {
        let src = "def f(a, b, c, d, e, g):\n    pass\n";
        assert_eq!(lint(&TooManyArguments, src).len(), 1);
    }

    #[test]
    fn conta_args_kwargs() {
        let src = "def f(a, b, c, d, *args, **kwargs):\n    pass\n";
        assert_eq!(lint(&TooManyArguments, src).len(), 1);
    }

    #[test]
    fn ignora_self_em_metodo() {
        let src = "\
class A:
    def m(self, a, b, c, d, e):
        pass
";
        assert_eq!(lint(&TooManyArguments, src).len(), 0);
    }

    #[test]
    fn ignora_cls_em_classmethod() {
        let src = "\
class A:
    @classmethod
    def criar(cls, a, b, c, d, e):
        pass
";
        assert_eq!(lint(&TooManyArguments, src).len(), 0);
    }

    #[test]
    fn self_conta_em_funcao_livre() {
        let src = "def f(self, a, b, c, d, e):\n    pass\n";
        assert_eq!(lint(&TooManyArguments, src).len(), 1);
    }

    #[test]
    fn self_conta_em_funcao_aninhada() {
        let src = "\
class A:
    def m(self):
        def inner(self, a, b, c, d, e):
            pass
";
        assert_eq!(lint(&TooManyArguments, src).len(), 1);
    }

    #[test]
    fn metodo_em_classe_aninhada_em_funcao() {
        let src = "\
def outer():
    class A:
        def m(self, a, b, c, d, e):
            pass
";
        assert_eq!(lint(&TooManyArguments, src).len(), 0);
    }

    #[test]
    fn respeita_config() {
        let src = "def f(a, b, c, d):\n    pass\n";
        let cfg = config_with_option("FOR003", "max-args", 3);
        assert_eq!(lint_with(&TooManyArguments, src, &cfg).len(), 1);
    }

    #[test]
    fn config_mais_permissiva() {
        let src = "def f(a, b, c, d, e, g, h):\n    pass\n";
        let cfg = config_with_option("FOR003", "max-args", 10);
        assert_eq!(lint_with(&TooManyArguments, src, &cfg).len(), 0);
    }

    #[test]
    fn conta_anotados_normalmente() {
        let src = "def f(a: int, b: str, c: float, d: bool, e: bytes, g: list):\n    pass\n";
        assert_eq!(lint(&TooManyArguments, src).len(), 1);
    }
}
