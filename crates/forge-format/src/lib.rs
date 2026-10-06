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

        // Só mexe em gaps puramente de espaços/tabs. Qualquer outra
        // coisa (comentário que escapou da árvore, string colada, etc.)
        // fica intocada.
        if !gap.chars().all(|c| c == ' ' || c == '\t') {
            continue;
        }

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

/// Coleta as folhas da árvore em ordem de byte.
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
/// ainda não cobre (keywords, indentação, colons, etc.).
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

    // 2. Operador unário seguido de operando — sem espaço.
    //    Tem precedência sobre binário porque `-1` deve ficar `-1`.
    if is_unary_op(prev) {
        return Some(String::new());
    }

    // 3. Operador binário — espaço ao redor.
    if is_binary_op(prev) || is_binary_op(next) {
        return Some(" ".to_string());
    }

    // 4. `=` em assignment ou `+=` etc em augmented_assignment.
    if is_assign_like_op(prev) || is_assign_like_op(next) {
        return Some(" ".to_string());
    }

    None
}

/// `x = -1` — o `-` aqui é unário. `x = a - b` — binário.
fn is_unary_op(n: Node) -> bool {
    if !matches!(n.kind(), "-" | "+" | "~") {
        return false;
    }
    matches!(n.parent().map(|p| p.kind()), Some("unary_operator"))
}

/// `+`, `-`, `*`, `**`, `/`, `//`, `%`, `@`, `&`, `|`, `^`, `<<`, `>>`
/// como operadores entre duas expressões; `==`, `!=`, `<`, `<=`, `>`, `>=`
/// como comparações.
///
/// **Não** pega `*` de `*args`/`**kwargs`: esses vivem em `list_splat_pattern`
/// e `dictionary_splat_pattern`, não em `binary_operator`.
fn is_binary_op(n: Node) -> bool {
    let Some(parent) = n.parent() else {
        return false;
    };
    if !matches!(parent.kind(), "binary_operator" | "comparison_operator") {
        return false;
    }
    matches!(
        n.kind(),
        "+" | "-"
            | "*"
            | "/"
            | "//"
            | "%"
            | "**"
            | "@"
            | "&"
            | "|"
            | "^"
            | "<<"
            | ">>"
            | "=="
            | "!="
            | "<"
            | "<="
            | ">"
            | ">="
    )
}

/// `x = 1` (assignment) ou `x += 1` (augmented_assignment).
fn is_assign_like_op(n: Node) -> bool {
    let Some(parent) = n.parent() else {
        return false;
    };
    match parent.kind() {
        "assignment" => n.kind() == "=",
        "augmented_assignment" => matches!(
            n.kind(),
            "+=" | "-="
                | "*="
                | "/="
                | "//="
                | "%="
                | "**="
                | "&="
                | "|="
                | "^="
                | ">>="
                | "<<="
                | "@="
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(s: &str) -> String {
        format_source(s).expect("parse falhou")
    }

    // ---- vírgulas ----

    #[test]
    fn adiciona_espaco_apos_virgula() {
        assert_eq!(fmt("f(a,b)\n"), "f(a, b)\n");
    }

    #[test]
    fn nao_espaca_antes_de_virgula() {
        assert_eq!(fmt("f(a , b)\n"), "f(a, b)\n");
    }

    #[test]
    fn lista_de_argumentos() {
        assert_eq!(fmt("f(a,b,c)\n"), "f(a, b, c)\n");
    }

    #[test]
    fn tupla_em_assignment() {
        assert_eq!(fmt("a,b = 1,2\n"), "a, b = 1, 2\n");
    }

    // ---- assignment ----

    #[test]
    fn adiciona_espaco_em_igual() {
        assert_eq!(fmt("x=1\n"), "x = 1\n");
    }

    #[test]
    fn preserva_igual_ja_espacado() {
        assert_eq!(fmt("x = 1\n"), "x = 1\n");
    }

    #[test]
    fn normaliza_multiplos_espacos() {
        assert_eq!(fmt("x   =   1\n"), "x = 1\n");
    }

    #[test]
    fn multiplas_atribuicoes() {
        assert_eq!(fmt("x=1\ny=2\nz=3\n"), "x = 1\ny = 2\nz = 3\n");
    }

    #[test]
    fn augmented_assignment() {
        assert_eq!(fmt("x+=1\n"), "x += 1\n");
    }

    // ---- default_parameter / keyword_argument: preservados ----

    #[test]
    fn default_parameter_sem_espaco() {
        assert_eq!(fmt("def f(x=1):\n    pass\n"), "def f(x=1):\n    pass\n");
    }

    #[test]
    fn keyword_argument_sem_espaco() {
        assert_eq!(fmt("g(x=1)\n"), "g(x=1)\n");
    }

    // ---- operadores binários ----

    #[test]
    fn espaco_em_soma() {
        assert_eq!(fmt("x=a+b\n"), "x = a + b\n");
    }

    #[test]
    fn espaco_em_multiplicacao() {
        assert_eq!(fmt("x=a*b\n"), "x = a * b\n");
    }

    #[test]
    fn espaco_em_comparacao() {
        assert_eq!(fmt("x=a==b\n"), "x = a == b\n");
    }

    #[test]
    fn espaco_em_menor_igual() {
        assert_eq!(fmt("x=a<=b\n"), "x = a <= b\n");
    }

    #[test]
    fn espaco_em_power() {
        assert_eq!(fmt("x=a**b\n"), "x = a ** b\n");
    }

    #[test]
    fn espaco_em_pipe() {
        assert_eq!(fmt("x=a|b\n"), "x = a | b\n");
    }

    // ---- unário: preserva `-1`, `+1`, `~x` ----

    #[test]
    fn unario_negativo_sem_espaco() {
        assert_eq!(fmt("x=-1\n"), "x = -1\n");
    }

    #[test]
    fn unario_positivo_sem_espaco() {
        assert_eq!(fmt("x=+1\n"), "x = +1\n");
    }

    #[test]
    fn unario_bit_not_sem_espaco() {
        assert_eq!(fmt("x=~y\n"), "x = ~y\n");
    }

    #[test]
    fn binario_com_unario_direita() {
        // `a - (-b)` — o segundo `-` é unário.
        assert_eq!(fmt("x=a - -b\n"), "x = a - -b\n");
    }

    // ---- *args / **kwargs preservados ----

    #[test]
    fn star_args_sem_alteracao() {
        assert_eq!(fmt("f(*args)\n"), "f(*args)\n");
    }

    #[test]
    fn double_star_kwargs_sem_alteracao() {
        assert_eq!(fmt("f(**kwargs)\n"), "f(**kwargs)\n");
    }

    #[test]
    fn def_star_args() {
        assert_eq!(
            fmt("def f(*args):\n    pass\n"),
            "def f(*args):\n    pass\n"
        );
    }

    // ---- preservação de contexto ----

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

    // ---- idempotência ----

    #[test]
    fn idempotente_simples() {
        let first = fmt("x=1\nf(a,b)\n");
        let second = fmt(&first);
        assert_eq!(first, second);
    }

    #[test]
    fn idempotente_com_operadores() {
        let first = fmt("x=a+b*c\n");
        let second = fmt(&first);
        assert_eq!(first, second);
    }

    #[test]
    fn idempotente_com_unario() {
        let first = fmt("x=-a-b\n");
        let second = fmt(&first);
        assert_eq!(first, second);
    }
}
