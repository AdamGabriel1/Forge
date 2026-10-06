use crate::util::{is_alone_on_line, line_end_with_newline, line_start_byte, walk};
use crate::Context;
use crate::Rule;
use forge_cfg::find_unreachable_in_block;
use forge_core::{Diagnostic, Edit, Severity};
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

    /// Remove cada statement inalcançável **se ele estiver sozinho na sua
    /// linha**. Statements multi-linha ou dividindo a linha com outro
    /// statement são pulados (mesma política conservadora do FOR006).
    fn fix(&self, node: Node, ctx: &Context, _diagnostics: &[Diagnostic]) -> Vec<Edit> {
        let bytes = ctx.source.as_bytes();
        let mut edits = Vec::new();

        walk(node, &mut |n| {
            if n.kind() != "block" {
                return;
            }
            let mut cursor = n.walk();
            let stmts: Vec<Node> = n.children(&mut cursor).filter(|c| c.is_named()).collect();

            let mut reached_terminator = false;
            for stmt in stmts {
                if !reached_terminator {
                    if forge_cfg::is_terminator(stmt) {
                        reached_terminator = true;
                    }
                    continue;
                }
                if !is_alone_on_line(stmt, ctx.source) {
                    continue;
                }
                let start = line_start_byte(stmt.start_byte(), bytes);
                let end = line_end_with_newline(stmt.end_byte(), bytes);
                edits.push(Edit::delete(start, end));
            }
        });

        edits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;
    use forge_core::Config;

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

    // ---- fix ----

    fn run_fix(source: &str) -> Vec<Edit> {
        let mut parser = forge_parser::get_parser();
        let tree = forge_parser::parse_python_source(&mut parser, source).unwrap();
        let cfg = forge_core::Config::default();
        let ctx = crate::Context::new(source, "<test>", &cfg, tree.root_node());
        let diags = UnreachableCode.check(tree.root_node(), &ctx);
        UnreachableCode.fix(tree.root_node(), &ctx, &diags)
    }

    #[test]
    fn fix_remove_linha_apos_return() {
        let src = "\
def f():
    return 1
    x = 2
";
        let edits = run_fix(src);
        assert_eq!(edits.len(), 1);
        let novo = forge_core::apply_edits(src, edits);
        assert_eq!(novo, "def f():\n    return 1\n");
    }

    #[test]
    fn fix_remove_multiplas_linhas() {
        let src = "\
def f():
    return 1
    x = 2
    y = 3
";
        let edits = run_fix(src);
        assert_eq!(edits.len(), 2);
        let novo = forge_core::apply_edits(src, edits);
        assert_eq!(novo, "def f():\n    return 1\n");
    }

    #[test]
    fn fix_nao_remove_codigo_reachable() {
        let src = "\
def f():
    x = 2
    return x
";
        assert!(run_fix(src).is_empty());
    }
}
