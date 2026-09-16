use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use merc_syntax::ConstructorId;
use merc_syntax::EqnSpecId;
use merc_syntax::EquationId;
use merc_syntax::MapId;
use merc_syntax::SortId;
use merc_syntax::Span;
use merc_syntax::UntypedDataSpecification;
use merc_syntax::VarId;

use crate::EquationTyping;
use crate::InferenceError;
use crate::ResolvedSortId;
use crate::Signature;
use crate::SortInterner;
use crate::TemplateCheck;
use crate::TemplateId;
use crate::TypingInfo;

/// The context shared by all type-checking queries.
///
/// It owns the [SortInterner] and one [HashMap] memoization table per query.
/// Each semantic fact is a memoized function on this context, so passes pull
/// their dependencies lazily and results are shared.
#[derive(Clone)]
pub(crate) struct TypeCheckContext {
    pub(crate) sorts: SortInterner,

    pub(crate) sort_of_def: HashMap<SortId, ResolvedSortId>,
    /// The memoized resolved sort of each constructor declaration, keyed by
    /// [ConstructorId]. Populated lazily by `query_sort_of_constructor`.
    pub(crate) sort_of_constructor: HashMap<ConstructorId, ResolvedSortId>,
    /// The memoized resolved sort of each map declaration, keyed by [MapId].
    /// Populated lazily by `query_sort_of_map`.
    pub(crate) sort_of_map: HashMap<MapId, ResolvedSortId>,
    /// The memoized resolved sort of each equation variable, keyed by its own [VarId].
    pub(crate) sort_of_equation_var: HashMap<VarId, ResolvedSortId>,

    /// The signature of the specification — one pooled table for every role (`User`/`Template`/
    /// `System` alike), consulted unfiltered regardless of which struct (if any) generated a given
    /// equation. See [`crate::EquationRole`]'s doc comment for why an earlier, per-struct-scoped
    /// version of this was removed.
    pub(crate) signature: Option<Arc<Signature>>,
    /// `(name, resolved sort) -> declaration span` for every system-defined constructor/mapping.
    pub(crate) system_symbol_spans: HashMap<(String, ResolvedSortId), Span>,

    /// The memoized results of `query_equation_typing`, keyed by the id of the
    /// enclosing equation specification block and the equation's own id
    /// within it.
    pub(crate) equation_typing: HashMap<(EqnSpecId, EquationId), Result<Arc<EquationTyping>, InferenceError>>,
    /// The system-equation counterpart of `equation_typing`.
    pub(crate) system_equation_typing: HashMap<(EqnSpecId, EquationId), Result<Arc<EquationTyping>, InferenceError>>,
    /// The proven typing of each Appendix-B container/function-update
    /// template's own equations, checked once with its type variable(s) held
    /// rigid by `check_template_equations`.
    pub(crate) template_typings: HashMap<TemplateId, TemplateCheck>,

    /// The memoized result of the public TypingInfo for every equation.
    pub(crate) equation_typing_info: HashMap<(EqnSpecId, EquationId), Arc<TypingInfo>>,
    /// The typing info for the whole set of equations.
    pub(crate) whole_typing_info: Option<Arc<TypingInfo>>,
}

impl TypeCheckContext {
    pub(crate) fn new() -> Self {
        TypeCheckContext {
            sorts: SortInterner::new(),
            sort_of_def: HashMap::new(),
            sort_of_constructor: HashMap::new(),
            sort_of_map: HashMap::new(),
            sort_of_equation_var: HashMap::new(),
            signature: None,
            system_symbol_spans: HashMap::new(),
            equation_typing: HashMap::new(),
            system_equation_typing: HashMap::new(),
            template_typings: HashMap::new(),
            equation_typing_info: HashMap::new(),
            whole_typing_info: None,
        }
    }
}

impl TypeCheckContext {
    /// Returns the memoized value for `key` in the cache selected by `cache`,
    /// computing and storing it via `compute` on a miss.
    ///
    /// `cache` projects `self` down to the relevant memoization [HashMap] and
    /// is re-applied on each access rather than borrowed once, so that
    /// `compute` can use `self` freely in between — including, recursively,
    /// other queries on `self`. Holding the projected `&mut HashMap` across
    /// that call would alias `self` and not compile.
    ///
    /// Every query built on this currently has no self-referential dependency (an equation's
    /// typing never depends on another equation's, and alias cycles are already rejected by
    /// `check_aliases` before `query_sort_of_def` ever recurses), so a query that did re-enter its
    /// own key would simply recompute rather than being caught — there is no cycle detection here.
    pub(crate) fn get_or_compute<K, V>(
        &mut self,
        cache: impl Fn(&mut Self) -> &mut HashMap<K, V>,
        key: K,
        compute: impl FnOnce(&mut Self) -> V,
    ) -> V
    where
        K: Eq + Hash + Clone,
        V: Clone,
    {
        if let Some(value) = cache(self).get(&key) {
            return value.clone();
        }

        let value = compute(self);
        cache(self).insert(key, value.clone());
        value
    }

    /// The declared name of the sort that [SortId] `def` resolves to — a user
    /// sort or a system-internal one such as `@NatPair` alike, both declared in
    /// `spec.sort_declarations` — or `None` when `def` is out of range.
    pub(crate) fn sort_name<'a>(&'a self, spec: &'a UntypedDataSpecification, def: SortId) -> Option<&'a str> {
        spec.sort_declarations.get(*def).map(|decl| decl.identifier.as_str())
    }

    /// As [`Self::sort_name`], but falls back to a synthesized `@sort_N` placeholder instead of
    /// `None` when `def` is out of range.
    pub(crate) fn sort_display_name<'a>(&'a self, spec: &'a UntypedDataSpecification, def: SortId) -> Cow<'a, str> {
        match self.sort_name(spec, def) {
            Some(name) => Cow::Borrowed(name),
            None => Cow::Owned(format!("@sort_{}", def.value())),
        }
    }
}

impl Default for TypeCheckContext {
    fn default() -> Self {
        TypeCheckContext::new()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use merc_syntax::SortId;

    use crate::ResolvedSortId;
    use crate::TypeCheckContext;

    #[test]
    fn test_get_or_compute_memoizes() {
        let mut ctx = TypeCheckContext::new();
        let key = SortId::new(1);
        let calls = Cell::new(0);

        let compute = |_: &mut TypeCheckContext| {
            calls.set(calls.get() + 1);
            ResolvedSortId::new(7)
        };
        let first = ctx.get_or_compute(|ctx| &mut ctx.sort_of_def, key, compute);
        let second = ctx.get_or_compute(|ctx| &mut ctx.sort_of_def, key, compute);

        assert_eq!(first, ResolvedSortId::new(7));
        assert_eq!(second, ResolvedSortId::new(7));
        assert_eq!(
            calls.get(),
            1,
            "the second lookup must hit the cache instead of recomputing"
        );
    }
}
