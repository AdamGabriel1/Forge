//! Formatter opinativo. Duas passadas:
//!
//! 1. **Gaps**: reescreve espaçamento entre tokens na mesma linha.
//! 2. **Indentação**: reescreve o whitespace inicial de cada
//!    statement, normalizando para 4 espaços por nível de bloco.
//!
//! Preserva multilinha, strings e comentários byte-a-byte dentro de
//! cada linha. Não toca continuations nem alinhamento interno de
//! expressões multi-linha.

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

/// Formata o fonte. Se nada muda, retorna o mesmo texto.
pub fn format_source(source: &str) -> Result<String, FormatError> {
    let stage1 = format_gaps(source)?;
    let stage2 = format_indentation(&stage1)?;
    Ok(stage2)
}

// ---------------------------------------------------------------------------
// Passo 1: espaçamento entre tokens
// ---------------------------------------------------------------------------

fn format_gaps(source: &str) -> Result<String, FormatError> {
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

        if gap.contains('\n') {
            continue;
        }
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

fn parent_is(n: Node, kind: &str) -> bool {
    n.parent().map(|p| p.kind() == kind).unwrap_or(false)
}

fn desired_gap(prev: Node, next: Node) -> Option<String> {
    // 1. `@` em decorator: colado ao nome.
    if prev.kind() == "@" && parent_is(prev, "decorator") {
        return Some(String::new());
    }

    // 2. `.` em attribute: sem espaço dos dois lados.
    if (prev.kind() == "." || next.kind() == ".") && parent_is(prev, "attribute") {
        return Some(String::new());
    }

    // 3. `(` de chamada: sem espaço antes.
    if next.kind() == "(" && parent_is(next, "arguments") {
        return Some(String::new());
    }

    // 4. `[` de subscript: sem espaço antes.
    if next.kind() == "[" && parent_is(next, "subscript") {
        return Some(String::new());
    }

    // 5. Abridores e fechadores: sem espaço nas bordas internas.
    if matches!(prev.kind(), "(" | "[" | "{") {
        return Some(String::new());
    }
    if matches!(next.kind(), ")" | "]" | "}") {
        return Some(String::new());
    }

    // 6. `:` — contexto decide.
    //    slice: sem espaço dos dois lados.
    //    pair / typed_parameter / typed_default_parameter: espaço depois,
    //    nenhum antes.
    if prev.kind() == ":" || next.kind() == ":" {
        if parent_is(prev, "slice") || parent_is(next, "slice") {
            return Some(String::new());
        }
        let is_colon_with_space = |n: Node| {
            n.kind() == ":"
                && matches!(
                    n.parent().map(|p| p.kind()),
                    Some("pair") | Some("typed_parameter") | Some("typed_default_parameter")
                )
        };
        if is_colon_with_space(prev) {
            return Some(" ".to_string());
        }
        if is_colon_with_space(next) {
            return Some(String::new());
        }
    }

    // 7. Vírgula.
    if prev.kind() == "," {
        if matches!(next.kind(), ")" | "]" | "}") {
            return Some(String::new());
        }
        return Some(" ".to_string());
    }
    if next.kind() == "," {
        return Some(String::new());
    }

    // 8. Walrus `:=` — espaço dos dois lados.
    if prev.kind() == ":=" || next.kind() == ":=" {
        return Some(" ".to_string());
    }

    // 9. `->` em anotação de retorno — espaço dos dois lados.
    if prev.kind() == "->" || next.kind() == "->" {
        return Some(" ".to_string());
    }

    // 10. Operador unário colado ao operando.
    if is_unary_op(prev) {
        return Some(String::new());
    }

    // 11. Operador binário — espaço dos dois lados.
    if is_binary_op(prev) || is_binary_op(next) {
        return Some(" ".to_string());
    }

    // 12. `=` e variantes.
    if is_assign_like_op(prev) || is_assign_like_op(next) {
        return Some(" ".to_string());
    }

    None
}

fn is_unary_op(n: Node) -> bool {
    matches!(n.kind(), "-" | "+" | "~") && parent_is(n, "unary_operator")
}

fn is_binary_op(n: Node) -> bool {
    let Some(parent) = n.parent() else {
        return false;
    };
    match parent.kind() {
        "binary_operator" | "comparison_operator" => matches!(
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
                | "in"
                | "not in"
                | "is"
                | "is not"
        ),
        "boolean_operator" => matches!(n.kind(), "and" | "or"),
        "not_operator" => n.kind() == "not",
        _ => false,
    }
}

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

// ---------------------------------------------------------------------------
// Passo 2: indentação
// ---------------------------------------------------------------------------

fn format_indentation(source: &str) -> Result<String, FormatError> {
    let mut parser = get_parser();
    let tree = parse_python_source(&mut parser, source).ok_or(FormatError::ParseFailed)?;

    let mut edits = Vec::new();
    walk_indent(tree.root_node(), 0, source, &mut edits);
    Ok(apply_edits(source, edits))
}

fn walk_indent(node: Node, depth: usize, source: &str, edits: &mut Vec<Edit>) {
    match node.kind() {
        "module" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    fix_stmt_indent(child, 0, source, edits);
                    walk_indent(child, 0, source, edits);
                }
            }
        }
        "block" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    fix_stmt_indent(child, depth, source, edits);
                    walk_indent(child, depth, source, edits);
                }
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if !child.is_named() {
                    continue;
                }
                if child.kind() == "block" {
                    walk_indent(child, depth + 1, source, edits);
                } else {
                    walk_indent(child, depth, source, edits);
                }
            }
        }
    }
}

fn fix_stmt_indent(node: Node, depth: usize, source: &str, edits: &mut Vec<Edit>) {
    let bytes = source.as_bytes();
    let start = node.start_byte();

    // Encontra o começo da linha onde o nó começa.
    let mut line_start = start;
    while line_start > 0 && bytes[line_start - 1] != b'\n' {
        line_start -= 1;
    }

    // Consome espaços/tabs iniciais.
    let mut ws_end = line_start;
    while ws_end < bytes.len() && (bytes[ws_end] == b' ' || bytes[ws_end] == b'\t') {
        ws_end += 1;
    }

    let current = &source[line_start..ws_end];
    let desired = "    ".repeat(depth);
    if current != desired {
        edits.push(Edit::replace(line_start, ws_end, desired));
    }
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

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

    // ---- default_parameter / keyword_argument ----

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

    // ---- booleanos e comparações especiais ----

    #[test]
    fn espaco_em_and() {
        assert_eq!(fmt("if a and b:\n    pass\n"), "if a and b:\n    pass\n");
    }

    #[test]
    fn espaco_em_or() {
        assert_eq!(fmt("if a or b:\n    pass\n"), "if a or b:\n    pass\n");
    }

    #[test]
    fn espaco_em_not() {
        assert_eq!(fmt("if not x:\n    pass\n"), "if not x:\n    pass\n");
    }

    #[test]
    fn is_none_espacado() {
        assert_eq!(
            fmt("if x is None:\n    pass\n"),
            "if x is None:\n    pass\n"
        );
    }

    #[test]
    fn in_espacado() {
        assert_eq!(fmt("if x in y:\n    pass\n"), "if x in y:\n    pass\n");
    }

    // ---- unário ----

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
        assert_eq!(fmt("x=a - -b\n"), "x = a - -b\n");
    }

    // ---- parênteses e colchetes ----

    #[test]
    fn remove_espaco_dentro_de_parenteses() {
        assert_eq!(fmt("f( a )\n"), "f(a)\n");
    }

    #[test]
    fn remove_espaco_dentro_de_lista() {
        assert_eq!(fmt("x = [ 1, 2 ]\n"), "x = [1, 2]\n");
    }

    #[test]
    fn remove_espaco_dentro_de_dict() {
        assert_eq!(fmt("x = { 1: 2 }\n"), "x = {1: 2}\n");
    }

    #[test]
    fn parenteses_vazios() {
        assert_eq!(fmt("x = f( )\n"), "x = f()\n");
    }

    #[test]
    fn colchetes_vazios() {
        assert_eq!(fmt("x = [ ]\n"), "x = []\n");
    }

    #[test]
    fn chaves_vazias() {
        assert_eq!(fmt("x = { }\n"), "x = {}\n");
    }

    // ---- chamada e subscript e attribute ----

    #[test]
    fn sem_espaco_antes_de_parentese_de_chamada() {
        assert_eq!(fmt("f (x)\n"), "f(x)\n");
    }

    #[test]
    fn sem_espaco_antes_de_colchete_de_subscript() {
        assert_eq!(fmt("x [1]\n"), "x[1]\n");
    }

    #[test]
    fn sem_espaco_em_attribute() {
        assert_eq!(fmt("x . y\n"), "x.y\n");
        assert_eq!(fmt("x. y\n"), "x.y\n");
        assert_eq!(fmt("x .y\n"), "x.y\n");
    }

    // ---- `:` em dict / anotação / slice ----

    #[test]
    fn espaco_em_dict_colon() {
        assert_eq!(fmt("x = {1:2}\n"), "x = {1: 2}\n");
    }

    #[test]
    fn espaco_em_typed_parameter() {
        assert_eq!(
            fmt("def f(x:int):\n    pass\n"),
            "def f(x: int):\n    pass\n"
        );
    }

    #[test]
    fn espaco_em_typed_default_parameter() {
        assert_eq!(
            fmt("def f(x:int=1):\n    pass\n"),
            "def f(x: int=1):\n    pass\n"
        );
    }

    #[test]
    fn slice_colon_sem_espaco() {
        assert_eq!(fmt("x = a[1:2]\n"), "x = a[1:2]\n");
    }

    // ---- walrus, arrow, decorator ----

    #[test]
    fn walrus_espacado() {
        assert_eq!(fmt("if (x:=1):\n    pass\n"), "if (x := 1):\n    pass\n");
    }

    #[test]
    fn arrow_espacado() {
        assert_eq!(
            fmt("def f()->int:\n    pass\n"),
            "def f() -> int:\n    pass\n"
        );
    }

    #[test]
    fn decorator_colado() {
        assert_eq!(
            fmt("@ decorator\ndef f():\n    pass\n"),
            "@decorator\ndef f():\n    pass\n"
        );
    }

    // ---- indentação ----

    #[test]
    fn reindenta_2_espacos_para_4() {
        let src = "def f():\n  x = 1\n  return x\n";
        let esperado = "def f():\n    x = 1\n    return x\n";
        assert_eq!(fmt(src), esperado);
    }

    #[test]
    fn reindenta_8_espacos_para_4() {
        let src = "def f():\n        x = 1\n";
        let esperado = "def f():\n    x = 1\n";
        assert_eq!(fmt(src), esperado);
    }

    #[test]
    fn reindenta_aninhado() {
        let src = "\
def f():
  if True:
        x = 1
        return x
";
        let esperado = "\
def f():
    if True:
        x = 1
        return x
";
        assert_eq!(fmt(src), esperado);
    }

    #[test]
    fn top_level_sem_indent() {
        let src = "x = 1\ny = 2\n";
        assert_eq!(fmt(src), src);
    }

    #[test]
    fn reindenta_else() {
        let src = "\
if x:
   a = 1
else:
   a = 2
";
        let esperado = "\
if x:
    a = 1
else:
    a = 2
";
        assert_eq!(fmt(src), esperado);
    }

    #[test]
    fn reindenta_try_except() {
        let src = "\
try:
   x = 1
except Exception:
   pass
";
        let esperado = "\
try:
    x = 1
except Exception:
    pass
";
        assert_eq!(fmt(src), esperado);
    }

    #[test]
    fn indentacao_ja_correta_nao_muda() {
        let src = "\
def f():
    if True:
        return 1
    return 0
";
        assert_eq!(fmt(src), src);
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

    #[test]
    fn star_args_sem_alteracao() {
        assert_eq!(fmt("f(*args)\n"), "f(*args)\n");
    }

    #[test]
    fn double_star_kwargs_sem_alteracao() {
        assert_eq!(fmt("f(**kwargs)\n"), "f(**kwargs)\n");
    }

    // ---- idempotência ----

    #[test]
    fn idempotente_simples() {
        let first = fmt("x=1\nf(a,b)\n");
        let second = fmt(&first);
        assert_eq!(first, second);
    }

    #[test]
    fn idempotente_operadores() {
        let first = fmt("x=a+b*c\n");
        let second = fmt(&first);
        assert_eq!(first, second);
    }

    #[test]
    fn idempotente_unario() {
        let first = fmt("x=-a-b\n");
        let second = fmt(&first);
        assert_eq!(first, second);
    }

    #[test]
    fn idempotente_parenteses() {
        let first = fmt("f( a , b )\n");
        let second = fmt(&first);
        assert_eq!(first, second);
    }

    #[test]
    fn idempotente_dict_colon() {
        let first = fmt("x={1:2}\n");
        let second = fmt(&first);
        assert_eq!(first, second);
    }

    #[test]
    fn idempotente_indentacao() {
        let first = fmt("def f():\n  x = 1\n  return x\n");
        let second = fmt(&first);
        assert_eq!(first, second);
    }
}
