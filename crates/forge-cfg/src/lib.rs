//! Análise de fluxo de controle e data-flow sobre a CST.

use forge_core::Range;
use tree_sitter::Node;

pub mod dataflow;
pub mod definite_assignment;
pub mod nullable;

/// `true` se o nó é uma instrução que encerra o fluxo linear do bloco.
pub fn is_terminator(node: Node) -> bool {
    matches!(
        node.kind(),
        "return_statement" | "raise_statement" | "break_statement" | "continue_statement"
    )
}

/// Retorna os ranges de todas as instruções diretas de um `block` que vêm
/// **depois** do primeiro terminador. Se não houver terminador, retorna vazio.
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