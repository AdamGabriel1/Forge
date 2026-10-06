use forge_core::{Config, Diagnostic, Edit};
use forge_semantic::SemanticModel;
use std::cell::OnceCell;
use tree_sitter::Node;

pub mod rules;
pub mod util;

use rules::*;

// ---------------------------------------------------------------------------
// Contexto de análise
// ---------------------------------------------------------------------------

/// Contexto passado para toda regra.
///
/// Carrega o texto-fonte, o caminho, a configuração e o nó raiz da árvore.
/// O `SemanticModel` é construído sob demanda e memoizado — regras que
/// precisam dele chamam `ctx.semantic()`, e as seguintes reutilizam o
/// mesmo modelo em vez de reanalisar a árvore do zero.
pub struct Context<'a> {
    pub source: &'a str,
    pub filepath: &'a str,
    pub config: &'a Config,
    root: Node<'a>,
    semantic: OnceCell<SemanticModel>,
}

impl<'a> Context<'a> {
    pub fn new(source: &'a str, filepath: &'a str, config: &'a Config, root: Node<'a>) -> Self {
        Self {
            source,
            filepath,
            config,
            root,
            semantic: OnceCell::new(),
        }
    }

    pub fn root(&self) -> Node<'a> {
        self.root
    }

    /// Retorna o modelo semântico, construindo-o na primeira chamada.
    pub fn semantic(&self) -> &SemanticModel {
        self.semantic
            .get_or_init(|| SemanticModel::analyze(self.root, self.source))
    }
}

// ---------------------------------------------------------------------------
// Trait Rule + Registry
// ---------------------------------------------------------------------------

pub trait Rule: Send + Sync {
    fn code(&self) -> &str;
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn fix_hint(&self) -> &str;
    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic>;

    /// Produz edits para corrigir os diagnósticos que **esta própria regra**
    /// gerou. Por padrão, nenhuma correção é oferecida.
    fn fix(&self, _node: Node, _ctx: &Context, _diagnostics: &[Diagnostic]) -> Vec<Edit> {
        Vec::new()
    }
}

pub struct RuleRegistry {
    rules: Vec<Box<dyn Rule>>,
}

impl RuleRegistry {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn register(&mut self, rule: Box<dyn Rule>) {
        self.rules.push(rule);
    }

    pub fn all(&self) -> impl Iterator<Item = &dyn Rule> {
        self.rules.iter().map(|r| r.as_ref())
    }

    pub fn find(&self, code: &str) -> Option<&dyn Rule> {
        self.rules
            .iter()
            .find(|r| r.code() == code)
            .map(|r| r.as_ref())
    }
}

impl Default for RuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub fn default_registry() -> RuleRegistry {
    let mut registry = RuleRegistry::new();
    registry.register(Box::new(BareExcept));
    registry.register(Box::new(MutableDefaultArgument));
    registry.register(Box::new(TooManyArguments));
    registry.register(Box::new(FunctionTooLong));
    registry.register(Box::new(ShadowedBuiltin));
    registry.register(Box::new(UnusedImport));
    registry.register(Box::new(UnusedVariable));
    registry.register(Box::new(ShadowedVariable));
    registry.register(Box::new(RedefinedFunction));
    registry.register(Box::new(UndefinedName));
    registry.register(Box::new(UnreachableCode));
    registry.register(Box::new(UsedBeforeAssignment));
    registry.register(Box::new(PossibleNoneDereference));
    registry.register(Box::new(ExpensiveOperationInsideLoop));
    registry.register(Box::new(ConstantCondition));
    registry
}
