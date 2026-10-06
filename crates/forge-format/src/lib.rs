//! Formatter MVP. Reescreve espaçamento em gaps de uma linha só;
//! preserva multilinha, strings e comentários byte-a-byte.

use forge_core::{apply_edits, Edit};
use forge_parser::{get_parser, parse_python_source};
use tree_sitter::Node;

#[derive(Debug)]
pub enum FormatError {
    ParseFailed,
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::ParseFailed => write!(f, "parser não conseguiu produzir uma árvore"),
        }
    }
}

impl std::error::Error for FormatError {}

/// Formata o fonte, retornando a nova versão. Se nada muda, retorna o
/// mesmo texto. Arquivo sem árvore válida retorna `Err(ParseFailed)`.
pub fn format_source(source: &str) -> Result<String, FormatError> {
    let mut parser = get_parser();
    let tree = parse_python_source(&mut parser, source).ok_or(FormatError::ParseFailed)?;

    let mut leaves: Vec<Node> = Vec::new();
    collect_leaves(tree.root_node(), &mut leaves);

    let mut edits = Vec::new();
    for window in leaves.windows(2) {
        let prev = window[0];
        let next = window[1];
        if prev.end_byte() > next.start_byte() {
            continue;
        }
        let gap_start = prev.end_byte();
        let gap_end = next.start_byte();
        let gap = &source[gap_start..gap_end];

        // Preserva multilinha.
        if gap.contains('\n') {
            continue;
        }

        // Não toca em gaps que envolvam comentários ou strings.
        if is_comment(prev) || is_comment(next) || in_string(prev) || in_string(next) {
            continue;
        }

        if let Some(desired) = desired_gap(prev, next) {
            if gap != desired {
                edits.push(Edit::replace(gap_start, gap_end, desired));
            }
        }
    }

    Ok(apply_edits(source, edits))
}

fn is_comment(n: Node) -> bool {
    n.kind() == "comment"
}

fn in_string(n: Node) -> bool {
    let mut current = Some(n);
    while let Some(c) = current {
        if c.kind() == "string" || c.kind() == "interpolation" {
            return true;
        }
        current = c.parent();
    }
    false
}

/// Coleta as folhas da árvore em ordem de byte (pré-ordem já é ordem
/// de byte no tree-sitter).
fn collect_leaves<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
    if node.child_count() == 0 {
        out.push(node);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_leaves(child, out);
    }
}

/// Decide o gap desejado entre duas folhas adjacentes.
///
/// `None` significa "deixe como está" — útil para casos que a v0
/// ainda não cobre (operadores, keywords, indentação).
fn desired_gap(prev: Node, next: Node) -> Option<String> {
    // 1. Vírgula: espaço depois (a menos que seguida de fechador).
    if prev.kind() == "," {
        if matches!(next.kind(), ")" | "]" | "}") {
            return Some(String::new());
        }
        return Some(" ".to_string());
    }
    if next.kind() == "," {
        return Some(String::new());
    }

    // 2. `=` em assignment: espaço de cada lado.
    //    `def f(x=1)` e `g(x=1)` (default_parameter / keyword_argument)
    //    NÃO são tocados.
    if is_assignment_eq(prev) || is_assignment_eq(next) {
        return Some(" ".to_string());
    }

    None
}

fn is_assignment_eq(n: Node) -> bool {
    if n.kind() != "=" {
        return false;
    }
    n.parent()
        .map(|p| p.kind() == "assignment")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(s: &str) -> String {
        format_source(s).expect("parse falhou")
    }

    #[test]
    fn adiciona_espaco_em_igual() {
        assert_eq!(fmt("x=1\n"), "x = 1\n");
    }

    #[test]
    fn preserva_igual_ja_espacado() {
        assert_eq!(fmt("x = 1\n"), "x = 1\n");
    }

    #[test]
    fn adiciona_espaco_apos_virgula() {
        assert_eq!(fmt("f(a,b)\n"), "f(a, b)\n");
    }

    #[test]
    fn nao_espaca_antes_de_virgula() {
        assert_eq!(fmt("f(a , b)\n"), "f(a, b)\n");
    }

    #[test]
    fn default_parameter_sem_espaco() {
        // `def f(x=1)` não é assignment, então não é tocado.
        assert_eq!(fmt("def f(x=1):\n    pass\n"), "def f(x=1):\n    pass\n");
    }

    #[test]
    fn keyword_argument_sem_espaco() {
        assert_eq!(fmt("g(x=1)\n"), "g(x=1)\n");
    }

    #[test]
    fn multilinha_preservado() {
        let src = "x = (\n    1,\n    2,\n)\n";
        assert_eq!(fmt(src), src);
    }

    #[test]
    fn comentario_preservado() {
        let src = "x = 1  # comentário\n";
        assert_eq!(fmt(src), src);
    }

    #[test]
    fn string_com_virgula_nao_quebra() {
        let src = "s = \"a,b\"\n";
        assert_eq!(fmt(src), "s = \"a,b\"\n");
    }

    #[test]
    fn multiplas_atribuicoes() {
        let src = "x=1\ny=2\nz=3\n";
        assert_eq!(fmt(src), "x = 1\ny = 2\nz = 3\n");
    }

    #[test]
    fn tupla_em_assignment() {
        let src = "a,b = 1,2\n";
        assert_eq!(fmt(src), "a, b = 1, 2\n");
    }

    #[test]
    fn lista_de_argumentos() {
        assert_eq!(fmt("f(a,b,c)\n"), "f(a, b, c)\n");
    }

    #[test]
    fn idempotente() {
        let first = fmt("x=1\nf(a,b)\n");
        let second = fmt(&first);
        assert_eq!(first, second);
    }
}
