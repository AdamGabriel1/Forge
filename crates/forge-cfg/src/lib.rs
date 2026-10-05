//! Análise de fluxo de controle linear sobre a CST.
//!
//! Nesta primeira versão, o crate expõe helpers que identificam
//! terminadores (`return`, `raise`, `break`, `continue`) e código
//! inalcançável em blocos lineares. Um CFG completo com nós e arestas
//! será adicionado quando chegarmos em `FOR013 possible_none_dereference`,
//! que exige análise de fluxo de dados real.

use forge_core::Range;
use tree_sitter::Node;

/// `true` se o nó é uma instrução que encerra o fluxo linear do bloco.
pub fn is_terminator(node: Node) -> bool {
    matches!(
        node.kind(),
        "return_statement" | "raise_statement" | "break_statement" | "continue_statement"
    )
}

/// Retorna os ranges de todas as instruções diretas de um `block`
/// que vêm **depois** do primeiro terminador. Se não houver terminador,
/// retorna vazio.
///
/// Só olha os filhos diretos. `if cond: return` não conta como terminador
/// do bloco externo, porque o `return` é condicional. Essa é a escolha
/// conservadora correta para evitar falsos positivos.
pub fn find_unreachable_in_block(block: Node) -> Vec<Range> {
    let mut cursor = block.walk();
    let stmts: Vec<Node> = block
        .children(&mut cursor)
        .filter(|c| c.is_named())
        .collect();

    let mut out = Vec::new();
    let mut reached_terminator = false;
    for stmt in stmts {
        if reached_terminator {
            out.push(range_of(stmt));
            continue;
        }
        if is_terminator(stmt) {
            reached_terminator = true;
        }
    }
    out
}

fn range_of(node: Node) -> Range {
    let start = node.start_position();
    let end = node.end_position();
    Range {
        start_line: start.row,
        start_col: start.column,
        end_line: end.row,
        end_col: end.column,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Os testes vivem no `forge-lint` porque exigem parse de Python.
    // Aqui só mantemos helpers puros.
    #[test]
    fn terminators_reconhecidos() {
        // Sem árvore, não testamos `is_terminator` diretamente. O
        // comportamento é coberto pelos testes de FOR011.
    }
}