use crate::util::{range_of, walk};
use crate::Rule;
use forge_core::{Context, Diagnostic, Severity};
use tree_sitter::Node;

pub struct MutableDefaultArgument;

impl Rule for MutableDefaultArgument {
    fn code(&self) -> &str {
        "FOR002"
    }
    fn name(&self) -> &str {
        "mutable_default_argument"
    }
    fn description(&self) -> &str {
        "Argumentos default mutáveis (`[]`, `{}`, `set()`) são compartilhados entre chamadas."
    }
    fn fix_hint(&self) -> &str {
        "Use `None` como default e inicialize dentro da função."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        walk(node, &mut |n| {
            if n.kind() == "default_parameter" || n.kind() == "typed_default_parameter" {
                if let Some(value) = n.child_by_field_name("value") {
                    if is_mutable_default(value, ctx.source) {
                        let name = n
                            .child_by_field_name("name")
                            .and_then(|id| id.utf8_text(ctx.source.as_bytes()).ok())
                            .unwrap_or("<parâmetro>");
                        diagnostics.push(Diagnostic::new(
                            "FOR002",
                            &format!(
                                "Argumento default mutável em `{}`. Use `None` e inicialize dentro da função.",
                                name
                            ),
                            range_of(value),
                            Severity::Warning,
                        ));
                    }
                }
            }
        });
        diagnostics
    }
}

fn is_mutable_default(value: Node, source: &str) -> bool {
    match value.kind() {
        "list" | "dictionary" | "set" => true,
        "call" => value
            .child_by_field_name("function")
            .and_then(|f| f.utf8_text(source.as_bytes()).ok())
            .map(|f| matches!(f, "list" | "dict" | "set"))
            .unwrap_or(false),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

    #[test]
    fn detecta_lista() {
        assert_eq!(
            lint(&MutableDefaultArgument, "def f(x=[]):\n    pass\n").len(),
            1
        );
    }

    #[test]
    fn detecta_dict() {
        assert_eq!(
            lint(&MutableDefaultArgument, "def f(x={}):\n    pass\n").len(),
            1
        );
    }

    #[test]
    fn detecta_set_literal() {
        assert_eq!(
            lint(&MutableDefaultArgument, "def f(x={1, 2}):\n    pass\n").len(),
            1
        );
    }

    #[test]
    fn detecta_chamada_list() {
        assert_eq!(
            lint(&MutableDefaultArgument, "def f(x=list()):\n    pass\n").len(),
            1
        );
    }

    #[test]
    fn ignora_none() {
        assert_eq!(
            lint(&MutableDefaultArgument, "def f(x=None):\n    pass\n").len(),
            0
        );
    }

    #[test]
    fn ignora_imutaveis() {
        let src = "def f(x=1, y='a', z=(1, 2), w=frozenset()):\n    pass\n";
        assert_eq!(lint(&MutableDefaultArgument, src).len(), 0);
    }

    #[test]
    fn detecta_em_metodo() {
        let src = "\
class A:
    def m(self, cache={}):
        pass
";
        assert_eq!(lint(&MutableDefaultArgument, src).len(), 1);
    }

    #[test]
    fn detecta_anotado() {
        assert_eq!(
            lint(&MutableDefaultArgument, "def f(x: list = []):\n    pass\n").len(),
            1
        );
    }
}