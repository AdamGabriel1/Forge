use crate::util::{range_of, walk};
use crate::Rule;
use forge_core::{Context, Diagnostic, Severity};
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
}