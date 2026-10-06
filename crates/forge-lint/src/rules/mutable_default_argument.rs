use crate::util::{range_of, walk};
use crate::Context;
use crate::Rule;
use forge_core::{Diagnostic, Edit, Severity};
use tree_sitter::Node;

pub struct MutableDefaultArgument;

impl Rule for MutableDefaultArgument {
    fn code(&self) -> &str {
        "FOR002"
    }
    fn name(&self) -> &str {
        "mutable_default_argument"
    }
    fn description(&self) -> &str {
        "Argumentos default mutáveis (`[]`, `{}`, `set()`) são compartilhados entre chamadas."
    }
    fn fix_hint(&self) -> &str {
        "Use `None` como default e inicialize dentro da função."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        walk(node, &mut |n| {
            if n.kind() == "default_parameter" || n.kind() == "typed_default_parameter" {
                if let Some(value) = n.child_by_field_name("value") {
                    if is_mutable_default(value, ctx.source) {
                        let name = n
                            .child_by_field_name("name")
                            .and_then(|id| id.utf8_text(ctx.source.as_bytes()).ok())
                            .unwrap_or("<parâmetro>");
                        diagnostics.push(Diagnostic::new(
                            "FOR002",
                            &format!(
                                "Argumento default mutável em `{}`. Use `None` e inicialize dentro da função.",
                                name
                            ),
                            range_of(value),
                            Severity::Warning,
                        ));
                    }
                }
            }
        });
        diagnostics
    }

    /// Aplica dois edits por ocorrência:
    ///
    /// 1. Substitui o literal mutável (`[]`, `{}`, `set()`, `list()`,
    ///    `dict()`) por `None` na assinatura.
    /// 2. Insere `if <name> is None: <name> = <literal>` **no início da
    ///    linha** do primeiro statement do corpo. O indent é detectado a
    ///    partir dessa linha, então funciona em funções de módulo (4) e
    ///    em métodos (8).
    fn fix(&self, node: Node, ctx: &Context, _diagnostics: &[Diagnostic]) -> Vec<Edit> {
        let bytes = ctx.source.as_bytes();
        let mut edits: Vec<Edit> = Vec::new();

        walk(node, &mut |n| {
            if n.kind() != "function_definition" {
                return;
            }
            let Some(params) = n.child_by_field_name("parameters") else {
                return;
            };
            let Some(body) = n.child_by_field_name("body") else {
                return;
            };

            // Coleta todos os parâmetros default mutáveis desta função.
            let mut fixes: Vec<(String, String)> = Vec::new();
            let mut cursor = params.walk();
            for p in params.children(&mut cursor) {
                if !matches!(p.kind(), "default_parameter" | "typed_default_parameter") {
                    continue;
                }
                let Some(value) = p.child_by_field_name("value") else {
                    continue;
                };
                if !is_mutable_default(value, ctx.source) {
                    continue;
                }
                let Some(name_node) = p.child_by_field_name("name") else {
                    continue;
                };
                let Ok(name) = name_node.utf8_text(bytes) else {
                    continue;
                };
                let Ok(literal) = value.utf8_text(bytes) else {
                    continue;
                };
                fixes.push((name.to_string(), literal.to_string()));
            }

            if fixes.is_empty() {
                return;
            }

            // 1. Substitui cada literal por `None`.
            let mut cursor = params.walk();
            for p in params.children(&mut cursor) {
                if !matches!(p.kind(), "default_parameter" | "typed_default_parameter") {
                    continue;
                }
                let Some(value) = p.child_by_field_name("value") else {
                    continue;
                };
                if !is_mutable_default(value, ctx.source) {
                    continue;
                }
                edits.push(Edit::replace(value.start_byte(), value.end_byte(), "None"));
            }

            // 2. Descobre início da linha do primeiro statement do corpo e
            //    o indent que essa linha usa.
            let line_start = first_body_line_start(body, bytes);
            let indent = detect_indent_at(line_start, bytes);

            let mut inserted = String::new();
            for (name, literal) in &fixes {
                inserted.push_str(&format!(
                    "{indent}if {name} is None:\n{indent}    {name} = {literal}\n",
                ));
            }
            edits.push(Edit::replace(line_start, line_start, inserted));
        });

        edits
    }
}

/// Retorna o offset do início da linha do primeiro statement do `block`.
fn first_body_line_start(body: Node, bytes: &[u8]) -> usize {
    let mut pos = body.start_byte();
    while pos < bytes.len() && bytes[pos] == b'\n' {
        pos += 1;
    }
    let mut ls = pos;
    while ls > 0 && bytes[ls - 1] != b'\n' {
        ls -= 1;
    }
    ls
}

/// Conta espaços e tabs consecutivos a partir de `line_start`.
fn detect_indent_at(line_start: usize, bytes: &[u8]) -> String {
    let mut i = line_start;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i == line_start {
        "    ".to_string()
    } else {
        String::from_utf8_lossy(&bytes[line_start..i]).to_string()
    }
}

fn is_mutable_default(value: Node, source: &str) -> bool {
    match value.kind() {
        "list" | "dictionary" | "set" => true,
        "call" => value
            .child_by_field_name("function")
            .and_then(|f| f.utf8_text(source.as_bytes()).ok())
            .map(|f| matches!(f, "list" | "dict" | "set"))
            .unwrap_or(false),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

    #[test]
    fn detecta_lista() {
        assert_eq!(
            lint(&MutableDefaultArgument, "def f(x=[]):\n    pass\n").len(),
            1
        );
    }

    #[test]
    fn detecta_dict() {
        assert_eq!(
            lint(&MutableDefaultArgument, "def f(x={}):\n    pass\n").len(),
            1
        );
    }

    #[test]
    fn detecta_set_literal() {
        assert_eq!(
            lint(&MutableDefaultArgument, "def f(x={1, 2}):\n    pass\n").len(),
            1
        );
    }

    #[test]
    fn detecta_chamada_list() {
        assert_eq!(
            lint(&MutableDefaultArgument, "def f(x=list()):\n    pass\n").len(),
            1
        );
    }

    #[test]
    fn ignora_none() {
        assert_eq!(
            lint(&MutableDefaultArgument, "def f(x=None):\n    pass\n").len(),
            0
        );
    }

    #[test]
    fn ignora_imutaveis() {
        let src = "def f(x=1, y='a', z=(1, 2), w=frozenset()):\n    pass\n";
        assert_eq!(lint(&MutableDefaultArgument, src).len(), 0);
    }

    #[test]
    fn detecta_em_metodo() {
        let src = "\
class A:
    def m(self, cache={}):
        pass
";
        assert_eq!(lint(&MutableDefaultArgument, src).len(), 1);
    }

    #[test]
    fn detecta_anotado() {
        assert_eq!(
            lint(&MutableDefaultArgument, "def f(x: list = []):\n    pass\n").len(),
            1
        );
    }

    // ---- fix ----

    fn run_fix(source: &str) -> Vec<Edit> {
        let mut parser = forge_parser::get_parser();
        let tree = forge_parser::parse_python_source(&mut parser, source).unwrap();
        let cfg = forge_core::Config::default();
        let ctx = crate::Context::new(source, "<test>", &cfg, tree.root_node());
        let diags = MutableDefaultArgument.check(tree.root_node(), &ctx);
        MutableDefaultArgument.fix(tree.root_node(), &ctx, &diags)
    }

    #[test]
    fn fix_lista_vazia_simples() {
        let src = "def f(x=[]):\n    print(x)\n";
        let edits = run_fix(src);
        let novo = forge_core::apply_edits(src, edits);
        assert_eq!(
            novo,
            "def f(x=None):\n    if x is None:\n        x = []\n    print(x)\n"
        );
    }

    #[test]
    fn fix_dict_vazio() {
        let src = "def f(opts={}):\n    return opts\n";
        let edits = run_fix(src);
        let novo = forge_core::apply_edits(src, edits);
        assert_eq!(
            novo,
            "def f(opts=None):\n    if opts is None:\n        opts = {}\n    return opts\n"
        );
    }

    #[test]
    fn fix_chamada_list() {
        let src = "def f(x=list()):\n    pass\n";
        let edits = run_fix(src);
        let novo = forge_core::apply_edits(src, edits);
        assert_eq!(
            novo,
            "def f(x=None):\n    if x is None:\n        x = list()\n    pass\n"
        );
    }

    #[test]
    fn fix_multiplos_params() {
        let src = "def f(a=[], b={}):\n    pass\n";
        let edits = run_fix(src);
        let novo = forge_core::apply_edits(src, edits);
        assert_eq!(
            novo,
            "def f(a=None, b=None):\n    if a is None:\n        a = []\n    if b is None:\n        b = {}\n    pass\n"
        );
    }

    #[test]
    fn fix_indentacao_de_metodo() {
        let src = "\
class A:
    def m(self, cache={}):
        return cache
";
        let edits = run_fix(src);
        let novo = forge_core::apply_edits(src, edits);
        assert_eq!(
            novo,
            "\
class A:
    def m(self, cache=None):
        if cache is None:
            cache = {}
        return cache
"
        );
    }

    #[test]
    fn fix_sem_alvo_nao_gera_edits() {
        let src = "def f(x=None, y=1):\n    pass\n";
        assert!(run_fix(src).is_empty());
    }
}
