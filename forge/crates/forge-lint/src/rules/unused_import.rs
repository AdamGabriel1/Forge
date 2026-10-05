use crate::util::{range_of, walk};
use crate::Rule;
use forge_core::{Context, Diagnostic, Severity};
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
}

fn collect_import_statement<'a>(
    stmt: Node<'a>,
    source: &str,
    out: &mut Vec<(Node<'a>, String)>,
) {
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

fn collect_import_from<'a>(
    stmt: Node<'a>,
    source: &str,
    out: &mut Vec<(Node<'a>, String)>,
) {
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
}