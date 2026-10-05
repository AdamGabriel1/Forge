use crate::util::range_of;
use crate::Rule;
use forge_core::{Context, Diagnostic, Range, Severity};
use tree_sitter::Node;

pub struct PossibleNoneDereference;

impl Rule for PossibleNoneDereference {
    fn code(&self) -> &str {
        "FOR013"
    }
    fn name(&self) -> &str {
        "possible_none_dereference"
    }
    fn description(&self) -> &str {
        "Uma variável atribuída a `None` é desreferenciada sem checagem — `AttributeError` em runtime."
    }
    fn fix_hint(&self) -> &str {
        "Adicione `if x is not None:` antes, ou atribua um valor não-None."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        visit_scopes(node, ctx.source, &mut diagnostics);
        diagnostics
    }
}

fn visit_scopes(node: Node, source: &str, out: &mut Vec<Diagnostic>) {
    match node.kind() {
        "module" | "function_definition" => {
            analyze_scope(node, source, out);
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                visit_scopes(child, source, out);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                visit_scopes(child, source, out);
            }
        }
    }
}

struct Assignment {
    name: String,
    line: usize,
    col: usize,
    is_none: bool,
}

struct Access {
    name: String,
    range: Range,
}

fn analyze_scope(scope_node: Node, source: &str, out: &mut Vec<Diagnostic>) {
    let body = if scope_node.kind() == "function_definition" {
        match scope_node.child_by_field_name("body") {
            Some(b) => b,
            None => return,
        }
    } else {
        scope_node
    };

    let mut assignments: Vec<Assignment> = Vec::new();
    let mut accesses: Vec<Access> = Vec::new();
    walk_collecting(body, source, &mut assignments, &mut accesses);

    if assignments.is_empty() || accesses.is_empty() {
        return;
    }

    for access in &accesses {
        let use_pos = (access.range.start_line, access.range.start_col);
        let last = assignments
            .iter()
            .filter(|a| a.name == access.name)
            .filter(|a| (a.line, a.col) < use_pos)
            .max_by_key(|a| (a.line, a.col));

        if let Some(a) = last {
            if a.is_none {
                out.push(Diagnostic::new(
                    "FOR013",
                    &format!(
                        "`{}` foi atribuído a `None` e depois desreferenciado — `AttributeError` em runtime.",
                        access.name
                    ),
                    access.range.clone(),
                    Severity::Warning,
                ));
            }
        }
    }
}

fn walk_collecting(
    node: Node,
    source: &str,
    assignments: &mut Vec<Assignment>,
    accesses: &mut Vec<Access>,
) {
    // Não descer em escopos aninhados.
    if matches!(
        node.kind(),
        "function_definition" | "class_definition" | "lambda"
    ) {
        return;
    }

    match node.kind() {
        "assignment" => {
            if let Some(left) = node.child_by_field_name("left") {
                if left.kind() == "identifier" {
                    if let Ok(name) = left.utf8_text(source.as_bytes()) {
                        let is_none = node
                            .child_by_field_name("right")
                            .map(|r| r.kind() == "none")
                            .unwrap_or(false);
                        let pos = left.start_position();
                        assignments.push(Assignment {
                            name: name.to_string(),
                            line: pos.row,
                            col: pos.column,
                            is_none,
                        });
                    }
                }
            }
        }
        "attribute" => {
            if let Some(object) = node.child_by_field_name("object") {
                if object.kind() == "identifier" {
                    if let Ok(name) = object.utf8_text(source.as_bytes()) {
                        accesses.push(Access {
                            name: name.to_string(),
                            range: range_of(object),
                        });
                    }
                }
            }
        }
        "subscript" => {
            if let Some(value) = node.child_by_field_name("value") {
                if value.kind() == "identifier" {
                    if let Ok(name) = value.utf8_text(source.as_bytes()) {
                        accesses.push(Access {
                            name: name.to_string(),
                            range: range_of(value),
                        });
                    }
                }
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_collecting(child, source, assignments, accesses);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

    #[test]
    fn none_assign_depois_attr() {
        let src = "\
def f():
    x = None
    x.foo()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 1);
    }

    #[test]
    fn none_assign_depois_subscript() {
        let src = "\
def f():
    x = None
    x[0]
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 1);
    }

    #[test]
    fn reatribuido_antes_do_uso_ok() {
        let src = "\
def f():
    x = None
    x = get_value()
    x.foo()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 0);
    }

    #[test]
    fn sem_atribuicao_none_ok() {
        let src = "\
def f():
    x = 1
    x.foo()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 0);
    }

    #[test]
    fn sem_deref_ok() {
        let src = "\
def f():
    x = None
    print(x)
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 0);
    }

    #[test]
    fn multiplos_derefs_apos_none() {
        let src = "\
def f():
    x = None
    x.foo
    x.bar
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 2);
    }

    #[test]
    fn modulo_scope() {
        let src = "\
x = None
x.foo()
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 1);
    }

    #[test]
    fn funcao_aninhada_nao_afeta() {
        // `x` da função externa não é o mesmo da interna — pulamos
        // escopos aninhados.
        let src = "\
def outer():
    x = None
    def inner():
        x = 1
        x.foo()
    return inner
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 0);
    }

    #[test]
    fn uso_antes_da_atribuicao_nao_flagra() {
        let src = "\
def f():
    x.foo()
    x = None
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 0);
    }

    #[test]
    fn attr_de_atribuicao_nao_conta() {
        // `x.foo = 5` — o LHS é attribute, não identifier; não vira
        // assignment de `x`, e o `x` do LHS será registrado como acesso.
        let src = "\
def f():
    x = None
    x.foo = 5
";
        assert_eq!(lint(&PossibleNoneDereference, src).len(), 1);
    }
}
