use crate::util::{range_of, walk};
use crate::Context;
use crate::Rule;
use forge_core::{Diagnostic, Edit, Severity};
use tree_sitter::Node;

pub struct BareExcept;

impl Rule for BareExcept {
    fn code(&self) -> &str {
        "FOR001"
    }
    fn name(&self) -> &str {
        "bare_except"
    }
    fn description(&self) -> &str {
        "Não use `except:` sozinho, pois captura SystemExit e KeyboardInterrupt."
    }
    fn fix_hint(&self) -> &str {
        "Use `except Exception:` ou exceções específicas."
    }

    fn check(&self, node: Node, _ctx: &Context) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        walk(node, &mut |n| {
            if n.kind() == "except_clause" && is_bare_except(n) {
                diagnostics.push(Diagnostic::new(
                    "FOR001",
                    "Uso de `except:` nu encontrado. Especifique a exceção (ex: `except Exception:`).",
                    range_of(n),
                    Severity::Warning,
                ));
            }
        });
        diagnostics
    }

    /// Substitui `except:` por `except Exception:` inserindo ` Exception`
    /// imediatamente antes do token `:`.
    fn fix(&self, node: Node, _ctx: &Context, _diagnostics: &[Diagnostic]) -> Vec<Edit> {
        let mut edits = Vec::new();
        walk(node, &mut |n| {
            if n.kind() != "except_clause" || !is_bare_except(n) {
                return;
            }
            let mut cursor = n.walk();
            for child in n.children(&mut cursor) {
                if child.kind() == ":" {
                    let offset = child.start_byte();
                    edits.push(Edit::replace(offset, offset, " Exception"));
                    break;
                }
            }
        });
        edits
    }
}

fn is_bare_except(node: Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == ":" {
            return true;
        }
        if child.is_named() {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;
    use forge_core::Config;

    #[test]
    fn detecta_bare_except() {
        let src = "try:\n    pass\nexcept:\n    pass\n";
        assert_eq!(lint(&BareExcept, src).len(), 1);
    }

    #[test]
    fn ignora_except_exception() {
        let src = "try:\n    pass\nexcept Exception:\n    pass\n";
        assert_eq!(lint(&BareExcept, src).len(), 0);
    }

    #[test]
    fn ignora_except_exception_as() {
        let src = "try:\n    pass\nexcept Exception as e:\n    pass\n";
        assert_eq!(lint(&BareExcept, src).len(), 0);
    }

    #[test]
    fn ignora_except_tupla() {
        let src = "try:\n    pass\nexcept (ValueError, TypeError):\n    pass\n";
        assert_eq!(lint(&BareExcept, src).len(), 0);
    }

    #[test]
    fn detecta_multiplos() {
        let src = "\
try:
    pass
except:
    pass
try:
    pass
except:
    pass
";
        assert_eq!(lint(&BareExcept, src).len(), 2);
    }

    #[test]
    fn detecta_com_finally() {
        let src = "\
try:
    pass
except:
    pass
finally:
    pass
";
        assert_eq!(lint(&BareExcept, src).len(), 1);
    }

    // ---- fix ----

    fn run_fix(source: &str) -> Vec<Edit> {
        let mut parser = forge_parser::get_parser();
        let tree = forge_parser::parse_python_source(&mut parser, source).unwrap();
        let cfg = forge_core::Config::default();
        let ctx = crate::Context::new(source, "<test>", &cfg, tree.root_node());
        let diags = BareExcept.check(tree.root_node(), &ctx);
        BareExcept.fix(tree.root_node(), &ctx, &diags)
    }

    #[test]
    fn fix_troca_except_nu_por_exception() {
        let src = "try:\n    pass\nexcept:\n    pass\n";
        let edits = run_fix(src);
        assert_eq!(edits.len(), 1);
        let novo = forge_core::apply_edits(src, edits);
        assert_eq!(novo, "try:\n    pass\nexcept Exception:\n    pass\n");
    }

    #[test]
    fn fix_troca_multiplos() {
        let src = "\
try:
    pass
except:
    pass
try:
    pass
except:
    pass
";
        let edits = run_fix(src);
        assert_eq!(edits.len(), 2);
        let novo = forge_core::apply_edits(src, edits);
        assert_eq!(
            novo,
            "\
try:
    pass
except Exception:
    pass
try:
    pass
except Exception:
    pass
"
        );
    }

    #[test]
    fn fix_nao_toca_except_ja_especificado() {
        let src = "try:\n    pass\nexcept Exception:\n    pass\n";
        assert!(run_fix(src).is_empty());
    }
}
