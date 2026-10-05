use forge_core::{Context, Diagnostic, Edit};
use tree_sitter::Node;

pub mod rules;
pub mod util;

use rules::*;

// ---------------------------------------------------------------------------
// Trait Rule + Registry
// ---------------------------------------------------------------------------

pub trait Rule {
    fn code(&self) -> &str;
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn fix_hint(&self) -> &str;
    fn check(&self, node: Node, ctx: &Context) -> Vec<Diagnostic>;

    /// Produz edits para corrigir os diagnósticos que **esta própria regra**
    /// gerou. Por padrão, nenhuma correção é oferecida.
    ///
    /// `diagnostics` contém apenas os diagnósticos produzidos por este
    /// `Rule::check` na mesma árvore/contexto.
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
    registry
}