use forge_core::Range;
use tree_sitter::Node;

/// Retorna o `Range` de um nó.
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
