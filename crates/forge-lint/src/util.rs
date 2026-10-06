use forge_core::Range;
use tree_sitter::Node;

// ---------------------------------------------------------------------------
// Range e traversal
// ---------------------------------------------------------------------------

pub(crate) fn range_of(node: Node) -> Range {
    let start = node.start_position();
    let end = node.end_position();
    Range {
        start_line: start.row,
        start_col: start.column,
        end_line: end.row,
        end_col: end.column,
    }
}

/// Percorre a árvore em pré-ordem.
///
/// O lifetime `'a` amarra a closure à mesma árvore do nó raiz, permitindo
/// armazenar `Node<'a>` em coleções que vivem além da chamada.
pub(crate) fn walk<'a, F: FnMut(Node<'a>)>(node: Node<'a>, f: &mut F) {
    f(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, f);
    }
}

// ---------------------------------------------------------------------------
// Helpers de linha (usados por regras que fazem autofix)
// ---------------------------------------------------------------------------

/// Offset do início da linha que contém `byte`.
pub(crate) fn line_start_byte(byte: usize, source: &[u8]) -> usize {
    let mut i = byte;
    while i > 0 && source[i - 1] != b'\n' {
        i -= 1;
    }
    i
}

/// Offset do fim da linha (excluindo `\n`) que contém `byte`.
pub(crate) fn line_end_byte(byte: usize, source: &[u8]) -> usize {
    let mut i = byte;
    while i < source.len() && source[i] != b'\n' {
        i += 1;
    }
    i
}

/// Offset do fim da linha incluindo o `\n`, se houver.
pub(crate) fn line_end_with_newline(byte: usize, source: &[u8]) -> usize {
    let end = line_end_byte(byte, source);
    if end < source.len() && source[end] == b'\n' {
        end + 1
    } else {
        end
    }
}

/// `true` se o nó ocupa uma única linha e nada mais (fora whitespace)
/// existe antes ou depois dele nessa linha.
pub(crate) fn is_alone_on_line(node: Node, source: &str) -> bool {
    if node.start_position().row != node.end_position().row {
        return false;
    }
    let bytes = source.as_bytes();
    let start = node.start_byte();
    let end = node.end_byte();
    let ls = line_start_byte(start, bytes);
    let le = line_end_byte(end, bytes);
    source[ls..start].trim().is_empty() && source[end..le].trim().is_empty()
}

// ---------------------------------------------------------------------------
// Helpers usados apenas pelos testes das regras
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod test_util {
    use crate::Rule;
    use forge_core::{Config, Context, Diagnostic};
    use forge_parser::{get_parser, parse_python_source};

    pub fn lint_with(rule: &dyn Rule, source: &str, config: &Config) -> Vec<Diagnostic> {
        let mut parser = get_parser();
        let tree = parse_python_source(&mut parser, source).expect("parse falhou");
        let ctx = Context {
            source,
            filepath: "<test>",
            config,
        };
        rule.check(tree.root_node(), &ctx)
    }

    pub fn lint(rule: &dyn Rule, source: &str) -> Vec<Diagnostic> {
        lint_with(rule, source, &Config::default())
    }

    pub fn config_with_option(code: &str, key: &str, value: i64) -> Config {
        let mut cfg = Config::default();
        let mut table = toml::value::Table::new();
        table.insert(key.to_string(), toml::Value::Integer(value));
        cfg.lint
            .options
            .insert(code.to_string(), toml::Value::Table(table));
        cfg
    }
}
