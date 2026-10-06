use crate::util::{range_of, walk};
use crate::Rule;
use forge_core::{Context, Diagnostic, Edit, Severity};
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
    /// 1. Substitui o literal mutável (`[]`, `{}`, `set()`) por `None`.
    /// 2. Insere `if <name> is None: <name> = <literal>` no início do
    ///    corpo da função.
    ///
    /// A inserção no corpo acontece **uma vez por parâmetro corrigido**,
    /// empilhando as guardas na mesma linha da primeira instrução. Não
    /// reformatamos indentação — isso é papel do formatter; aqui só
    /// garantimos que o código roda igual.
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
            let mut fixes: Vec<(String, String)> = Vec::new(); // (nome, literal)
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
                edits.push(Edit::replace(
                    value.start_byte(),
                    value.end_byte(),
                    "None",
                ));
            }

            // 2. Empilha guardas no início do corpo.
            let body_start = body.start_byte();
            let indent = detect_body_indent(body, ctx.source);
            let mut inserted = String::new();
            for (name, literal) in &fixes {
                inserted.push_str(&format!(
                    "{indent}if {name} is None:\n{indent}    {name} = {literal}\n",
                ));
            }
            edits.push(Edit::replace(body_start, body_start, inserted));
        });

        edits
    }
}

/// Detecta a indentação usada pela primeira instrução do corpo.
/// Se o `block` estiver vazio, usa 4 espaços.
fn detect_body_indent(body: Node, source: &str) -> String {
    let bytes = source.as_bytes();
    let start = body.start_byte();
    let mut i = start;
    // Anda para frente até achar o começo da linha seguinte ao `:`
    // (o `block` começa logo depois da quebra de linha).
    while i < bytes.len() && bytes[i] == b'\n' {
        i += 1;
    }
    let line_start = i;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    let indent = &source[line_start..i];
    if indent.is_empty() {
        "    ".to_string()
    } else {
        indent.to_string()
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
    use forge_core::Config;

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
        let cfg = Config::default();
        let ctx = Context {
            source,
            filepath: "<test>",
            config: &cfg,
        };
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