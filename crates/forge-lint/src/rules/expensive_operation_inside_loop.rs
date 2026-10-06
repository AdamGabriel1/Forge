use crate::util::{range_of, walk};
use crate::Rule;
use forge_core::{Context, Diagnostic, Severity};
use tree_sitter::Node;

pub struct ExpensiveOperationInsideLoop;

impl Rule for ExpensiveOperationInsideLoop {
    fn code(&self) -> &str {
        "FOR014"
    }
    fn name(&self) -> &str {
        "expensive_operation_inside_loop"
    }
    fn description(&self) -> &str {
        "Operações caras (`sorted`, `reversed`, `.sort()`, comprehensions) dentro de loops são reexecutadas a cada iteração."
    }
    fn fix_hint(&self) -> &str {
        "Se o resultado não muda entre iterações, pré-compute fora do loop. Caso contrário, considere algoritmos alternativos."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        walk(node, &mut |n| {
            if !matches!(n.kind(), "for_statement" | "while_statement") {
                return;
            }
            if let Some(body) = n.child_by_field_name("body") {
                check_body(body, ctx.source, &mut diagnostics);
            }
        });
        diagnostics
    }
}

/// Percorre o corpo do loop. Se encontrar um loop aninhado, verifica sua
/// **expressão iterada** (que é reavaliada a cada iteração externa), mas
/// **não desce no corpo** — isso é feito quando o `walk` externo visitar
/// o próprio loop aninhado. Sem isso, `reversed(x)` no cabeçalho de um
/// `for` interno nunca seria visto.
fn check_body(node: Node, source: &str, out: &mut Vec<Diagnostic>) {
    if matches!(node.kind(), "for_statement" | "while_statement") {
        match node.kind() {
            "for_statement" => {
                if let Some(right) = node.child_by_field_name("right") {
                    check_body(right, source, out);
                }
            }
            "while_statement" => {
                if let Some(cond) = node.child_by_field_name("condition") {
                    check_body(cond, source, out);
                }
            }
            _ => {}
        }
        return;
    }

    if let Some(reason) = expensive_reason(node, source) {
        out.push(Diagnostic::new(
            "FOR014",
            &format!("{} dentro de loop — reexecutado a cada iteração.", reason),
            range_of(node),
            Severity::Warning,
        ));
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        check_body(child, source, out);
    }
}

fn expensive_reason(node: Node, source: &str) -> Option<&'static str> {
    match node.kind() {
        "list_comprehension" => Some("List comprehension"),
        "set_comprehension" => Some("Set comprehension"),
        "dictionary_comprehension" => Some("Dict comprehension"),
        "call" => {
            let func = node.child_by_field_name("function")?;
            match func.kind() {
                "identifier" => {
                    let name = func.utf8_text(source.as_bytes()).ok()?;
                    match name {
                        "sorted" => Some("`sorted()`"),
                        "reversed" => Some("`reversed()`"),
                        _ => None,
                    }
                }
                "attribute" => {
                    let attr = func.child_by_field_name("attribute")?;
                    let name = attr.utf8_text(source.as_bytes()).ok()?;
                    if name == "sort" {
                        Some("`.sort()`")
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

    #[test]
    fn sorted_dentro_de_for() {
        let src = "\
for x in items:
    print(sorted(x))
";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 1);
    }

    #[test]
    fn sorted_fora_do_loop_ok() {
        let src = "\
y = sorted(items)
for x in y:
    print(x)
";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 0);
    }

    #[test]
    fn sort_method_dentro_de_for() {
        let src = "\
for x in items:
    x.sort()
";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 1);
    }

    #[test]
    fn reversed_dentro_de_for() {
        let src = "\
for x in items:
    for y in reversed(x):
        print(y)
";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 1);
    }

    #[test]
    fn comprehension_dentro_de_loop() {
        let src = "\
for x in items:
    y = [i * 2 for i in x]
";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 1);
    }

    #[test]
    fn set_comprehension_dentro_de_loop() {
        let src = "\
for x in items:
    y = {i * 2 for i in x}
";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 1);
    }

    #[test]
    fn dict_comprehension_dentro_de_loop() {
        let src = "\
for x in items:
    y = {i: i * 2 for i in x}
";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 1);
    }

    #[test]
    fn while_com_sorted() {
        let src = "\
while cond():
    print(sorted(xs))
";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 1);
    }

    #[test]
    fn loops_aninhados_contam_uma_vez() {
        let src = "\
for a in xs:
    for b in ys:
        print(sorted(b))
";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 1);
    }

    #[test]
    fn operacao_no_loop_externo_e_no_interno() {
        let src = "\
for a in xs:
    print(sorted(a))
    for b in ys:
        print(sorted(b))
";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 2);
    }

    #[test]
    fn chamadas_leves_ok() {
        let src = "\
for x in items:
    print(x)
    y = x + 1
";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 0);
    }

    #[test]
    fn sem_loop_nao_reporta() {
        let src = "print(sorted(xs))\n";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 0);
    }

    #[test]
    fn multiplas_operacoes_no_mesmo_loop() {
        let src = "\
for x in items:
    a = sorted(x)
    b = reversed(x)
    c = [i for i in x]
";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 3);
    }

    #[test]
    fn comprehension_no_nivel_do_modulo_nao_conta() {
        let src = "xs = [i * 2 for i in items]\n";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 0);
    }

    #[test]
    fn operacao_no_else_do_loop_nao_conta() {
        let src = "\
for x in items:
    print(x)
else:
    print(sorted(items))
";
        assert_eq!(lint(&ExpensiveOperationInsideLoop, src).len(), 0);
    }
}
