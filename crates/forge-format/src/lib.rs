//! Formatter opinativo. Três passadas:
//!
//! 1. **Gaps**: espaçamento entre tokens na mesma linha.
//! 2. **Indentação**: bloco, normalizando para 4 espaços por nível.
//! 3. **Continuations**: linhas dentro de `()`/`[]`/`{}` multi-linha,
//!    normalizadas para `base + 4 * profundidade`.
//!
//! Preserva multilinha de strings, comentários, e backslash continuations.

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

pub fn format_source(source: &str) -> Result<String, FormatError> {
    let s1 = format_gaps(source)?;
    let s2 = format_indentation(&s1)?;
    let s3 = format_continuations(&s2)?;
    Ok(s3)
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
    // `@` em decorator.
    if prev.kind() == "@" && parent_is(prev, "decorator") {
        return Some(String::new());
    }

    // `.` em attribute.
    if (prev.kind() == "." || next.kind() == ".") && parent_is(prev, "attribute") {
        return Some(String::new());
    }

    // `(` de chamada/params.
    if next.kind() == "(" {
        if let Some(p) = next.parent() {
            if matches!(
                p.kind(),
                "argument_list" | "arguments" | "call" | "parameters"
            ) {
                return Some(String::new());
            }
        }
    }

    // `[` de subscript.
    if next.kind() == "[" && parent_is(next, "subscript") {
        return Some(String::new());
    }

    // Abridores e fechadores.
    if matches!(prev.kind(), "(" | "[" | "{") {
        return Some(String::new());
    }
    if matches!(next.kind(), ")" | "]" | "}") {
        return Some(String::new());
    }

    // `:` no fim de cabeçalho de bloco.
    if next.kind() == ":" {
        if let Some(p) = next.parent() {
            if matches!(
                p.kind(),
                "if_statement"
                    | "for_statement"
                    | "while_statement"
                    | "with_statement"
                    | "try_statement"
                    | "except_clause"
                    | "finally_clause"
                    | "else_clause"
                    | "function_definition"
                    | "class_definition"
            ) {
                return Some(String::new());
            }
        }
    }

    // `:` contextual.
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

    // Vírgula.
    if prev.kind() == "," {
        if matches!(next.kind(), ")" | "]" | "}") {
            return Some(String::new());
        }
        return Some(" ".to_string());
    }
    if next.kind() == "," {
        return Some(String::new());
    }

    // Walrus, arrow.
    if prev.kind() == ":=" || next.kind() == ":=" {
        return Some(" ".to_string());
    }
    if prev.kind() == "->" || next.kind() == "->" {
        return Some(" ".to_string());
    }

    // Unário.
    if is_unary_op(prev) {
        return Some(String::new());
    }

    // Binário.
    if is_binary_op(prev) || is_binary_op(next) {
        return Some(" ".to_string());
    }

    // Assignment.
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
// Passo 2: indentação de blocos
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

    let mut line_start = start;
    while line_start > 0 && bytes[line_start - 1] != b'\n' {
        line_start -= 1;
    }

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
// Passo 3: indentação de continuations
// ---------------------------------------------------------------------------

fn format_continuations(source: &str) -> Result<String, FormatError> {
    let mut parser = get_parser();
    let tree = parse_python_source(&mut parser, source).ok_or(FormatError::ParseFailed)?;

    let mut edits = Vec::new();
    walk_continuations(tree.root_node(), source, &mut edits);
    Ok(apply_edits(source, edits))
}

fn walk_continuations(node: Node, source: &str, edits: &mut Vec<Edit>) {
    if matches!(node.kind(), "module" | "block") {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if !child.is_named() || child.kind() == "comment" {
                continue;
            }
            if is_compound(child) {
                // Recurse to find nested blocks.
                walk_continuations(child, source, edits);
            } else {
                fix_statement_continuations(child, source, edits);
            }
        }
    } else {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.is_named() {
                walk_continuations(child, source, edits);
            }
        }
    }
}

fn is_compound(node: Node) -> bool {
    matches!(
        node.kind(),
        "if_statement"
            | "for_statement"
            | "while_statement"
            | "with_statement"
            | "try_statement"
            | "function_definition"
            | "class_definition"
            | "match_statement"
            | "decorated_definition"
    )
}

fn fix_statement_continuations(stmt: Node, source: &str, edits: &mut Vec<Edit>) {
    let start_row = stmt.start_position().row;
    let end_row = stmt.end_position().row;
    if start_row == end_row {
        return;
    }

    let bytes = source.as_bytes();
    let stmt_start = stmt.start_byte();
    let stmt_end = stmt.end_byte();

    // Base indent = whitespace at start of statement's first line.
    let mut line_start = stmt_start;
    while line_start > 0 && bytes[line_start - 1] != b'\n' {
        line_start -= 1;
    }
    let base_indent = stmt_start - line_start;

    // Collect brackets in byte order.
    let mut brackets: Vec<(usize, u8)> = Vec::new();
    collect_brackets_in_order(stmt, &mut brackets);
    brackets.sort_by_key(|(b, _)| *b);

    // Collect string ranges to skip.
    let mut strings: Vec<(usize, usize)> = Vec::new();
    collect_strings(stmt, &mut strings);

    // Find first newline after stmt_start.
    let mut i = stmt_start;
    while i < stmt_end && bytes[i] != b'\n' {
        i += 1;
    }
    if i >= stmt_end {
        return;
    }
    i += 1;

    while i < stmt_end {
        let mut line_end = i;
        while line_end < stmt_end && bytes[line_end] != b'\n' {
            line_end += 1;
        }

        // Skip if inside a string.
        let mut ws_end = i;
        while ws_end < line_end && (bytes[ws_end] == b' ' || bytes[ws_end] == b'\t') {
            ws_end += 1;
        }

        if ws_end >= line_end {
            // Blank line — clear whitespace.
            if ws_end > i {
                edits.push(Edit::replace(i, ws_end, ""));
            }
            i = if line_end < stmt_end {
                line_end + 1
            } else {
                stmt_end
            };
            continue;
        }

        if is_inside_string(ws_end, &strings) {
            i = if line_end < stmt_end {
                line_end + 1
            } else {
                stmt_end
            };
            continue;
        }

        // Compute bracket depth at line start.
        let mut depth: isize = 0;
        for (bpos, bkind) in &brackets {
            if *bpos >= i {
                break;
            }
            match *bkind {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                _ => {}
            }
        }
        // If depth is 0, this is a backslash continuation — skip.
        if depth <= 0 {
            i = if line_end < stmt_end {
                line_end + 1
            } else {
                stmt_end
            };
            continue;
        }

        let first_content_byte = bytes[ws_end];
        let mut effective_depth = depth as usize;
        if matches!(first_content_byte, b')' | b']' | b'}') {
            effective_depth = effective_depth.saturating_sub(1);
        }

        let desired_indent = base_indent + effective_depth * 4;
        let current_indent = ws_end - i;
        if current_indent != desired_indent {
            edits.push(Edit::replace(i, ws_end, " ".repeat(desired_indent)));
        }

        i = if line_end < stmt_end {
            line_end + 1
        } else {
            stmt_end
        };
    }
}

fn collect_brackets_in_order(node: Node, out: &mut Vec<(usize, u8)>) {
    match node.kind() {
        "(" | ")" | "[" | "]" | "{" | "}" => {
            out.push((node.start_byte(), node.kind().as_bytes()[0]));
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_brackets_in_order(child, out);
    }
}

fn collect_strings(node: Node, out: &mut Vec<(usize, usize)>) {
    if node.kind() == "string" {
        out.push((node.start_byte(), node.end_byte()));
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_strings(child, out);
    }
}

fn is_inside_string(byte: usize, strings: &[(usize, usize)]) -> bool {
    // Considera "dentro" apenas se o byte está estritamente entre start e
    // end. O primeiro byte de uma string **não** conta como dentro —
    // se a linha começa exatamente com o primeiro char de uma string
    // (ex: `    'a': 1,`), ela deve ser reformatada normalmente.
    strings
        .iter()
        .any(|(start, end)| byte > *start && byte < *end)
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

    // ---- booleanos ----

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
    fn sem_espaco_antes_de_parentese_de_parametros() {
        assert_eq!(fmt("def f ():\n    pass\n"), "def f():\n    pass\n");
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

    // ---- `:` ----

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

    #[test]
    fn remove_espaco_antes_de_colon_em_if() {
        assert_eq!(fmt("if x :\n    pass\n"), "if x:\n    pass\n");
    }

    #[test]
    fn remove_espaco_antes_de_colon_em_def() {
        assert_eq!(fmt("def f() :\n    pass\n"), "def f():\n    pass\n");
    }

    #[test]
    fn remove_espaco_antes_de_colon_em_else() {
        assert_eq!(
            fmt("if x:\n    pass\nelse :\n    pass\n"),
            "if x:\n    pass\nelse:\n    pass\n"
        );
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

    // ---- indentação de bloco ----

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

    // ---- continuations (novo) ----

    #[test]
    fn continuation_corrige_indent_em_chamada() {
        let src = "x = foo(\n        a,\n        b,\n)\n";
        let esperado = "x = foo(\n    a,\n    b,\n)\n";
        assert_eq!(fmt(src), esperado);
    }

    #[test]
    fn continuation_ja_correta_nao_muda() {
        let src = "x = foo(\n    a,\n    b,\n)\n";
        assert_eq!(fmt(src), src);
    }

    #[test]
    fn continuation_em_funcao() {
        let src = "def f():\n    x = foo(\n            a,\n            b,\n    )\n";
        let esperado = "def f():\n    x = foo(\n        a,\n        b,\n    )\n";
        assert_eq!(fmt(src), esperado);
    }

    #[test]
    fn continuation_aninhada() {
        let src = "x = foo(\n        bar(\n                a,\n        ),\n)\n";
        let esperado = "x = foo(\n    bar(\n        a,\n    ),\n)\n";
        assert_eq!(fmt(src), esperado);
    }

    #[test]
    fn continuation_com_lista() {
        let src = "x = [\n        1,\n        2,\n]\n";
        let esperado = "x = [\n    1,\n    2,\n]\n";
        assert_eq!(fmt(src), esperado);
    }

    #[test]
    fn continuation_com_dict() {
        let src = "x = {\n        'a': 1,\n        'b': 2,\n}\n";
        let esperado = "x = {\n    'a': 1,\n    'b': 2,\n}\n";
        assert_eq!(fmt(src), esperado);
    }

    #[test]
    fn continuation_fechador_alinhado() {
        let src = "x = foo(\n    a,\n        )\n";
        let esperado = "x = foo(\n    a,\n)\n";
        assert_eq!(fmt(src), esperado);
    }

    #[test]
    fn continuation_string_multilinha_preservada() {
        let src = "x = \"\"\"\n    indented\n    string\n\"\"\"\n";
        assert_eq!(fmt(src), src);
    }

    #[test]
    fn continuation_backslash_preservada() {
        let src = "x = 1 + \\\n    2\n";
        assert_eq!(fmt(src), src);
    }

    #[test]
    fn continuation_idempotente() {
        let first = fmt("x = foo(\n        a,\n        b,\n)\n");
        let second = fmt(&first);
        assert_eq!(first, second);
    }

    // ---- preservação de contexto ----

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

    #[test]
    fn def_star_args() {
        assert_eq!(
            fmt("def f(*args):\n    pass\n"),
            "def f(*args):\n    pass\n"
        );
    }

    // ---- idempotência geral ----

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

    #[test]
    fn idempotente_com_parenteses() {
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

    #[test]
    fn idempotente_chamadas() {
        let first = fmt("f (x) . y\n");
        let second = fmt(&first);
        assert_eq!(first, second);
    }
}
