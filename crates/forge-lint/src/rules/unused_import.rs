use crate::util::{range_of, walk};
use crate::Rule;
use forge_core::{Context, Diagnostic, Edit, Severity};
use std::collections::HashSet;
use tree_sitter::Node;

pub struct UnusedImport;

impl Rule for UnusedImport {
    fn code(&self) -> &str {
        "FOR006"
    }
    fn name(&self) -> &str {
        "unused_import"
    }
    fn description(&self) -> &str {
        "Imports que nunca são usados deveriam ser removidos."
    }
    fn fix_hint(&self) -> &str {
        "Remova o import. Se for intencional (ex: re-export), marque com `# noqa: FOR006`."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let mut imports: Vec<(Node, String)> = Vec::new();
        walk(node, &mut |n| match n.kind() {
            "import_statement" => collect_import_statement(n, ctx.source, &mut imports),
            "import_from_statement" => collect_import_from(n, ctx.source, &mut imports),
            _ => {}
        });

        if imports.is_empty() {
            return Vec::new();
        }

        let mut used: HashSet<String> = HashSet::new();
        collect_used(node, ctx.source, &mut used);

        let mut diagnostics = Vec::new();
        for (id_node, name) in imports {
            if !used.contains(&name) {
                diagnostics.push(Diagnostic::new(
                    "FOR006",
                    &format!("Import `{}` nunca é usado.", name),
                    range_of(id_node),
                    Severity::Warning,
                ));
            }
        }
        diagnostics
    }

    /// Estratégia conservadora: só remove a linha inteira quando
    /// **todos** os nomes daquela instrução de import estão sem uso e a
    /// instrução é a única coisa na sua linha. Casos como
    /// `from x import a, b` com só `a` sem uso precisariam reescrever a
    /// lista de nomes — fica para quando tivermos edições parciais.
    fn fix(&self, node: Node, ctx: &Context, _diagnostics: &[Diagnostic]) -> Vec<Edit> {
        let source = ctx.source;
        let bytes = source.as_bytes();

        // Reúne (statement, nomes declarados) para cada import do arquivo.
        let mut stmts: Vec<(Node, Vec<String>)> = Vec::new();
        walk(node, &mut |n| match n.kind() {
            "import_statement" => {
                let mut list = Vec::new();
                collect_import_statement(n, source, &mut list);
                if !list.is_empty() {
                    stmts.push((n, list.into_iter().map(|(_, name)| name).collect()));
                }
            }
            "import_from_statement" => {
                let mut list = Vec::new();
                collect_import_from(n, source, &mut list);
                if !list.is_empty() {
                    stmts.push((n, list.into_iter().map(|(_, name)| name).collect()));
                }
            }
            _ => {}
        });

        let mut used: HashSet<String> = HashSet::new();
        collect_used(node, source, &mut used);

        let mut edits = Vec::new();
        for (stmt, names) in stmts {
            // Todos os nomes sem uso?
            if !names.iter().all(|n| !used.contains(n)) {
                continue;
            }
            // A instrução está sozinha na linha?
            if !is_only_statement_on_line(stmt, source) {
                continue;
            }

            let start = stmt.start_byte();
            let mut end = stmt.end_byte();

            // Come também trailing whitespace + newline.
            while end < bytes.len() && (bytes[end] == b' ' || bytes[end] == b'\t') {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b'\n' {
                end += 1;
            } else if end + 1 < bytes.len() && bytes[end] == b'\r' && bytes[end + 1] == b'\n' {
                end += 2;
            }

            edits.push(Edit::delete(start, end));
        }

        edits
    }
}

/// `true` se o nó é a única coisa na sua linha (só whitespace antes e depois).
/// Restrito a nós de uma única linha — não tentamos corrigir imports
/// multi-linha por enquanto.
fn is_only_statement_on_line(node: Node, source: &str) -> bool {
    let start_row = node.start_position().row;
    let end_row = node.end_position().row;
    if start_row != end_row {
        return false;
    }

    let bytes = source.as_bytes();
    let start_byte = node.start_byte();
    let end_byte = node.end_byte();

    let mut line_start = start_byte;
    while line_start > 0 && bytes[line_start - 1] != b'\n' {
        line_start -= 1;
    }

    let mut line_end = end_byte;
    while line_end < bytes.len() && bytes[line_end] != b'\n' {
        line_end += 1;
    }

    source[line_start..start_byte].trim().is_empty() && source[end_byte..line_end].trim().is_empty()
}

fn collect_import_statement<'a>(stmt: Node<'a>, source: &str, out: &mut Vec<(Node<'a>, String)>) {
    let mut cursor = stmt.walk();
    for child in stmt.children(&mut cursor) {
        match child.kind() {
            "dotted_name" => {
                let name = first_segment(child, source);
                out.push((child, name));
            }
            "aliased_import" => {
                if let Some(alias) = child.child_by_field_name("alias") {
                    if let Ok(name) = alias.utf8_text(source.as_bytes()) {
                        out.push((alias, name.to_string()));
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_import_from<'a>(stmt: Node<'a>, source: &str, out: &mut Vec<(Node<'a>, String)>) {
    let mut seen_module = false;
    let mut cursor = stmt.walk();
    for child in stmt.children(&mut cursor) {
        match child.kind() {
            "relative_import" => {
                seen_module = true;
            }
            "dotted_name" => {
                if !seen_module {
                    seen_module = true;
                } else {
                    out.push((child, first_segment(child, source)));
                }
            }
            "aliased_import" => {
                if let Some(alias) = child.child_by_field_name("alias") {
                    if let Ok(name) = alias.utf8_text(source.as_bytes()) {
                        out.push((alias, name.to_string()));
                    }
                }
            }
            "wildcard_import" => {}
            _ => {}
        }
    }
}

fn first_segment(node: Node, source: &str) -> String {
    match node.utf8_text(source.as_bytes()) {
        Ok(text) => text.split('.').next().unwrap_or(text).to_string(),
        Err(_) => String::new(),
    }
}

fn collect_used(node: Node, source: &str, used: &mut HashSet<String>) {
    if node.kind() == "import_statement" || node.kind() == "import_from_statement" {
        return;
    }
    if node.kind() == "identifier" {
        if let Ok(text) = node.utf8_text(source.as_bytes()) {
            used.insert(text.to_string());
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_used(child, source, used);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

    #[test]
    fn import_simples_nao_usado() {
        assert_eq!(lint(&UnusedImport, "import os\n").len(), 1);
    }

    #[test]
    fn import_simples_usado() {
        let src = "import os\nprint(os.getcwd())\n";
        assert_eq!(lint(&UnusedImport, src).len(), 0);
    }

    #[test]
    fn import_com_dotted_usado() {
        let src = "import os.path\nprint(os.path.join('a', 'b'))\n";
        assert_eq!(lint(&UnusedImport, src).len(), 0);
    }

    #[test]
    fn import_com_dotted_nao_usado() {
        assert_eq!(lint(&UnusedImport, "import os.path\n").len(), 1);
    }

    #[test]
    fn from_import_usado() {
        let src = "from os import path\nprint(path.join('a', 'b'))\n";
        assert_eq!(lint(&UnusedImport, src).len(), 0);
    }

    #[test]
    fn from_import_nao_usado() {
        assert_eq!(lint(&UnusedImport, "from os import path\n").len(), 1);
    }

    #[test]
    fn from_import_aliased_usado() {
        let src = "from os import path as p\nprint(p.join('a', 'b'))\n";
        assert_eq!(lint(&UnusedImport, src).len(), 0);
    }

    #[test]
    fn from_import_aliased_nao_usado() {
        assert_eq!(lint(&UnusedImport, "from os import path as p\n").len(), 1);
    }

    #[test]
    fn import_aliased_usado() {
        let src = "import os as o\nprint(o.getcwd())\n";
        assert_eq!(lint(&UnusedImport, src).len(), 0);
    }

    #[test]
    fn import_aliased_nao_usado() {
        assert_eq!(lint(&UnusedImport, "import os as o\n").len(), 1);
    }

    #[test]
    fn wildcard_nao_reporta() {
        assert_eq!(lint(&UnusedImport, "from os import *\n").len(), 0);
    }

    #[test]
    fn multiplos_parcialmente_usados() {
        let src = "import os, sys\nprint(os.getcwd())\n";
        assert_eq!(lint(&UnusedImport, src).len(), 1);
    }

    #[test]
    fn from_com_multiplos_parcialmente_usados() {
        let src = "from os import path, sep\nprint(path.join('a', 'b'))\n";
        assert_eq!(lint(&UnusedImport, src).len(), 1);
    }

    #[test]
    fn anotacao_de_tipo_conta_como_uso() {
        let src = "from typing import List\nx: List[int] = []\n";
        assert_eq!(lint(&UnusedImport, src).len(), 0);
    }

    #[test]
    fn import_em_decorator() {
        let src = "\
import functools
@functools.cache
def f():
    pass
";
        assert_eq!(lint(&UnusedImport, src).len(), 0);
    }

    // ---- fix ----

    fn make_ctx<'a>(source: &'a str, config: &'a forge_core::Config) -> Context<'a> {
        Context {
            source,
            filepath: "<test>",
            config,
        }
    }

    fn run_fix(source: &str) -> Vec<Edit> {
        let mut parser = forge_parser::get_parser();
        let tree = forge_parser::parse_python_source(&mut parser, source).unwrap();
        let cfg = forge_core::Config::default();
        let ctx = make_ctx(source, &cfg);
        let diags = UnusedImport.check(tree.root_node(), &ctx);
        UnusedImport.fix(tree.root_node(), &ctx, &diags)
    }

    #[test]
    fn fix_remove_linha_de_import_simples() {
        let src = "import os\nimport sys\nprint(sys.version)\n";
        let edits = run_fix(src);
        assert_eq!(edits.len(), 1);

        let novo = forge_core::apply_edits(src, edits);
        assert_eq!(novo, "import sys\nprint(sys.version)\n");
    }

    #[test]
    fn fix_nao_remove_linha_com_import_misto() {
        // `import os, sys` com só `os` sem uso não é reescrito.
        let src = "import os, sys\nprint(sys.version)\n";
        let edits = run_fix(src);
        assert!(edits.is_empty());
    }

    #[test]
    fn fix_nao_remove_linha_com_dois_statements() {
        let src = "import os; import sys\nprint(sys.version)\n";
        let edits = run_fix(src);
        assert!(edits.is_empty());
    }

    #[test]
    fn fix_remove_dois_imports_independentes() {
        let src = "import os\nimport sys\nimport json\nprint(sys.version)\n";
        let edits = run_fix(src);
        assert_eq!(edits.len(), 2);

        let novo = forge_core::apply_edits(src, edits);
        assert_eq!(novo, "import sys\nprint(sys.version)\n");
    }
}
