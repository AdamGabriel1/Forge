use tree_sitter::{Parser, Tree};

pub fn get_parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::language())
        .expect("Erro ao carregar linguagem Python");
    parser
}

pub fn parse_python_source(parser: &mut Parser, source: &str) -> Option<Tree> {
    parser.parse(source, None)
}
