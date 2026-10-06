use crate::util::{range_of, walk};
use crate::Context;
use crate::Rule;
use forge_core::{Diagnostic, Severity};
use tree_sitter::Node;

const PYTHON_BUILTINS: &[&str] = &[
    "abs",
    "aiter",
    "all",
    "anext",
    "any",
    "ascii",
    "bin",
    "bool",
    "breakpoint",
    "bytearray",
    "bytes",
    "callable",
    "chr",
    "classmethod",
    "compile",
    "complex",
    "delattr",
    "dict",
    "dir",
    "divmod",
    "enumerate",
    "eval",
    "exec",
    "filter",
    "float",
    "format",
    "frozenset",
    "getattr",
    "globals",
    "hasattr",
    "hash",
    "help",
    "hex",
    "id",
    "input",
    "int",
    "isinstance",
    "issubclass",
    "iter",
    "len",
    "list",
    "locals",
    "map",
    "max",
    "memoryview",
    "min",
    "next",
    "object",
    "oct",
    "open",
    "ord",
    "pow",
    "print",
    "property",
    "range",
    "repr",
    "reversed",
    "round",
    "set",
    "setattr",
    "slice",
    "sorted",
    "staticmethod",
    "str",
    "sum",
    "super",
    "tuple",
    "type",
    "vars",
    "zip",
    "Ellipsis",
    "NotImplemented",
    "ArithmeticError",
    "AssertionError",
    "AttributeError",
    "BaseException",
    "BlockingIOError",
    "BrokenPipeError",
    "BufferError",
    "BytesWarning",
    "ChildProcessError",
    "ConnectionAbortedError",
    "ConnectionError",
    "ConnectionRefusedError",
    "ConnectionResetError",
    "DeprecationWarning",
    "EOFError",
    "Exception",
    "FileExistsError",
    "FileNotFoundError",
    "FloatingPointError",
    "FutureWarning",
    "GeneratorExit",
    "ImportError",
    "ImportWarning",
    "IndentationError",
    "IndexError",
    "InterruptedError",
    "IsADirectoryError",
    "KeyError",
    "KeyboardInterrupt",
    "LookupError",
    "MemoryError",
    "ModuleNotFoundError",
    "NameError",
    "NotADirectoryError",
    "NotImplementedError",
    "OSError",
    "OverflowError",
    "PendingDeprecationWarning",
    "PermissionError",
    "ProcessLookupError",
    "RecursionError",
    "ReferenceError",
    "ResourceWarning",
    "RuntimeError",
    "RuntimeWarning",
    "StopAsyncIteration",
    "StopIteration",
    "SyntaxError",
    "SyntaxWarning",
    "SystemError",
    "SystemExit",
    "TabError",
    "TimeoutError",
    "TypeError",
    "UnboundLocalError",
    "UnicodeDecodeError",
    "UnicodeEncodeError",
    "UnicodeError",
    "UnicodeTranslateError",
    "UnicodeWarning",
    "UserWarning",
    "ValueError",
    "Warning",
    "ZeroDivisionError",
];

pub struct ShadowedBuiltin;

impl Rule for ShadowedBuiltin {
    fn code(&self) -> &str {
        "FOR005"
    }
    fn name(&self) -> &str {
        "shadowed_builtin"
    }
    fn description(&self) -> &str {
        "Sobrescrever um builtin do Python (ex: `list = []`) causa bugs difíceis de rastrear."
    }
    fn fix_hint(&self) -> &str {
        "Renomeie a variável/função/classe para algo que não colida com builtins."
    }

    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        walk(node, &mut |n| match n.kind() {
            "assignment" => {
                if let Some(left) = n.child_by_field_name("left") {
                    if left.kind() == "identifier" {
                        check_builtin(left, ctx, &mut diagnostics);
                    }
                }
            }
            "function_definition" | "class_definition" => {
                if let Some(name) = n.child_by_field_name("name") {
                    check_builtin(name, ctx, &mut diagnostics);
                }
            }
            "for_statement" => {
                if let Some(left) = n.child_by_field_name("left") {
                    if left.kind() == "identifier" {
                        check_builtin(left, ctx, &mut diagnostics);
                    }
                }
            }
            _ => {}
        });
        diagnostics
    }
}

pub(crate) fn is_builtin(name: &str) -> bool {
    PYTHON_BUILTINS.contains(&name)
}

fn check_builtin(node: Node, ctx: &Context, diagnostics: &mut Vec<Diagnostic>) {
    let Ok(name) = node.utf8_text(ctx.source.as_bytes()) else {
        return;
    };
    if is_builtin(name) {
        diagnostics.push(Diagnostic::new(
            "FOR005",
            &format!("`{}` sobrescreve um builtin do Python.", name),
            range_of(node),
            Severity::Warning,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_util::lint;

    #[test]
    fn detecta_assign_list() {
        assert_eq!(lint(&ShadowedBuiltin, "list = []\n").len(), 1);
    }

    #[test]
    fn detecta_assign_dict() {
        assert_eq!(lint(&ShadowedBuiltin, "dict = {}\n").len(), 1);
    }

    #[test]
    fn detecta_function_def() {
        assert_eq!(lint(&ShadowedBuiltin, "def list():\n    pass\n").len(), 1);
    }

    #[test]
    fn detecta_class_def() {
        assert_eq!(lint(&ShadowedBuiltin, "class int:\n    pass\n").len(), 1);
    }

    #[test]
    fn detecta_for_loop() {
        assert_eq!(
            lint(&ShadowedBuiltin, "for list in []:\n    pass\n").len(),
            1
        );
    }

    #[test]
    fn detecta_excecao() {
        assert_eq!(lint(&ShadowedBuiltin, "Exception = 1\n").len(), 1);
    }

    #[test]
    fn ignora_nome_normal() {
        assert_eq!(lint(&ShadowedBuiltin, "x = 1\ny = 2\n").len(), 0);
    }

    #[test]
    fn ignora_nome_parecido() {
        assert_eq!(
            lint(&ShadowedBuiltin, "my_list = []\nmy_dict = {}\n").len(),
            0
        );
    }

    #[test]
    fn detecta_multiplos() {
        let src = "list = []\ndict = {}\n";
        assert_eq!(lint(&ShadowedBuiltin, src).len(), 2);
    }
}
