use crate::util::{range_of, walk};
use crate::Context;
use crate::Rule;
use forge_core::{Diagnostic, Severity};
use tree_sitter::Node;

const DEFAULT_MAX_LINES: usize = 50;

pub struct FunctionTooLong;

impl Rule for FunctionTooLong {
    fn code(&self) -> &str {
        "FOR004"
    }
    fn name(&self) -> &str {
        "function_too_long"
    }
    fn description(&self) -> &str {
        "Funções muito longas são difíceis de entender, testar e manter."
    }
    fn fix_hint(&self) -> &str {
        "Extraia partes da função em funções menores com responsabilidades claras."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let max_lines = ctx
            .config
            .lint
            .option_usize("FOR004", "max-lines")
            .unwrap_or(DEFAULT_MAX_LINES);

        let mut diagnostics = Vec::new();
        walk(node, &mut |n| {
            if n.kind() != "function_definition" {
                return;
            }
            let Some(body) = n.child_by_field_name("body") else {
                return;
            };
            let start = body.start_position().row;
            let end = body.end_position().row;
            let lines = end.saturating_sub(start) + 1;
            if lines > max_lines {
                let name = n
                    .child_by_field_name("name")
                    .and_then(|id| id.utf8_text(ctx.source.as_bytes()).ok())
                    .unwrap_or("<função>");
                diagnostics.push(Diagnostic::new(
                    "FOR004",
                    &format!(
                        "Função `{}` tem {} linhas (máximo: {}).",
                        name, lines, max_lines
                    ),
                    range_of(n),
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
    use crate::util::test_util::{config_with_option, lint, lint_with};

    #[test]
    fn funcao_curta_nao_e_flagrada() {
        let src = "def f():\n    a = 1\n    b = 2\n";
        assert_eq!(lint(&FunctionTooLong, src).len(), 0);
    }

    #[test]
    fn funcao_longa_e_flagrada() {
        let src = "\
def f():
    a = 1
    b = 2
    c = 3
";
        let cfg = config_with_option("FOR004", "max-lines", 2);
        assert_eq!(lint_with(&FunctionTooLong, src, &cfg).len(), 1);
    }

    #[test]
    fn respeita_config() {
        let src = "\
def f():
    a = 1
    b = 2
    c = 3
    d = 4
";
        let cfg = config_with_option("FOR004", "max-lines", 10);
        assert_eq!(lint_with(&FunctionTooLong, src, &cfg).len(), 0);
    }

    #[test]
    fn funcao_vazia_nao_e_flagrada() {
        let src = "def f():\n    pass\n";
        assert_eq!(lint(&FunctionTooLong, src).len(), 0);
    }

    #[test]
    fn default_50_aceita_funcao_de_10_linhas() {
        let src = "\
def f():
    a = 1
    b = 2
    c = 3
    d = 4
    e = 5
    g = 6
    h = 7
    i = 8
    j = 9
";
        assert_eq!(lint(&FunctionTooLong, src).len(), 0);
    }
}
