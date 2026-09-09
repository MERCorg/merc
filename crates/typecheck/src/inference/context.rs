use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::Hash;
use std::sync::Arc;

use merc_syntax::ConstructorId;
use merc_syntax::DefId;
use merc_syntax::EqnSpecId;
use merc_syntax::EquationId;
use merc_syntax::MapId;
use merc_syntax::Span;
use merc_syntax::UntypedDataSpecification;
use merc_syntax::VarId;

use crate::EquationTyping;
use crate::InferenceError;
use crate::PolySortScheme;
use crate::ResolvedSortId;
use crate::Signature;
use crate::SortInterner;
use crate::TypingInfo;

/// The context shared by all type-checking queries.
///
/// It owns the [SortInterner] and one [QueryCache] per query. Each semantic
/// fact is a memoized function on this context, so passes pull their
/// dependencies lazily and results are shared.
pub(crate) struct TypeCheckContext {
    pub(crate) sorts: SortInterner,

    pub(crate) sort_of_def: QueryCache<DefId, ResolvedSortId>,
    /// The memoized resolved sort of each constructor declaration, keyed by
    /// [ConstructorId]. Populated lazily by `query_sort_of_constructor`.
    pub(crate) sort_of_constructor: QueryCache<ConstructorId, ResolvedSortId>,
    /// The memoized resolved sort of each map declaration, keyed by [MapId].
    /// Populated lazily by `query_sort_of_map`.
    pub(crate) sort_of_map: QueryCache<MapId, ResolvedSortId>,
    /// The memoized resolved sort of each equation variable, keyed by its own [VarId].
    pub(crate) sort_of_equation_var: QueryCache<VarId, ResolvedSortId>,

    /// The signature of the specification.
    pub(crate) signature: Option<Arc<Signature>>,
    /// The resolved signature of the *basic-sort* part of the system-defined
    /// specification; containers are deliberately excluded, see
    /// `resolve_system_signature`.
    pub(crate) system_signature: Option<Arc<Signature>>,
    /// The signature a system equation's body is checked against, indexed by
    /// its enclosing block's `EqnSpecId`. Scoped per
    /// [`crate::SystemEquationGroup`] rather than pooled, see that type.
    pub(crate) system_equation_signature_by_group: Vec<Arc<Signature>>,
    /// The system-internal sort name table, needed to resolve a `Reference`
    /// sort (e.g. `@NatPair`) while checking a system equation.
    pub(crate) system_sort_ids: Option<Arc<HashMap<String, ResolvedSortId>>>,
    /// The narrow polymorphic scheme table (comparison operators and `if`
    /// only) a system equation's own body is checked against.
    pub(crate) builtin_scheme_signature: Option<Arc<HashMap<String, Vec<PolySortScheme>>>>,
    /// `(name, resolved sort) -> declaration span` for every system-defined constructor/mapping.
    pub(crate) system_symbol_spans: HashMap<(String, ResolvedSortId), Span>,

    /// The memoized results of `query_equation_typing`, keyed by the id of the
    /// enclosing equation specification block and the equation's own id
    /// within it.
    pub(crate) equation_typing: QueryCache<(EqnSpecId, EquationId), Result<Arc<EquationTyping>, InferenceError>>,
    /// The system-equation counterpart of `equation_typing`.
    pub(crate) system_equation_typing: QueryCache<(EqnSpecId, EquationId), Result<Arc<EquationTyping>, InferenceError>>,

    /// The memoized result of the public TypingInfo for every equation.
    pub(crate) equation_typing_info: QueryCache<(EqnSpecId, EquationId), Arc<TypingInfo>>,
    /// The typing info for the whole set of equations.
    pub(crate) whole_typing_info: Option<Arc<TypingInfo>>,
}

impl TypeCheckContext {
    pub(crate) fn new() -> Self {
        TypeCheckContext {
            sorts: SortInterner::new(),
            sort_of_def: QueryCache::new(),
            sort_of_constructor: QueryCache::new(),
            sort_of_map: QueryCache::new(),
            sort_of_equation_var: QueryCache::new(),
            signature: None,
            system_signature: None,
            builtin_scheme_signature: None,
            system_equation_signature_by_group: Vec::new(),
            system_sort_ids: None,
            system_symbol_spans: HashMap::new(),
            equation_typing: QueryCache::new(),
            system_equation_typing: QueryCache::new(),
            equation_typing_info: QueryCache::new(),
            whole_typing_info: None,
        }
    }
}

impl TypeCheckContext {
    /// Returns the memoized value for `key` in the cache selected by `cache`,
    /// computing and storing it via `compute` on a miss. Re-entering `key`
    /// from within `compute` (a query depending on itself) fails with
    /// [CyclicQuery] instead of recursing unboundedly.
    ///
    /// `cache` projects `self` down to the relevant [QueryCache] and is
    /// re-applied on each access rather than borrowed once, so that `compute`
    /// can use `self` freely in between — including, recursively, other
    /// queries on `self`. Holding the projected `&mut QueryCache` across that
    /// call would alias `self` and not compile.
    pub(crate) fn get_or_compute<K, V>(
        &mut self,
        cache: impl Fn(&mut Self) -> &mut QueryCache<K, V>,
        key: K,
        compute: impl FnOnce(&mut Self) -> V,
    ) -> Result<V, CyclicQuery>
    where
        K: Eq + Hash + Clone,
        V: Clone,
    {
        match cache(self).entries.entry(key.clone()) {
            Entry::Occupied(entry) => {
                return match entry.get() {
                    QueryEntry::Done(value) => Ok(value.clone()),
                    QueryEntry::InProgress => Err(CyclicQuery),
                };
            }
            Entry::Vacant(entry) => {
                entry.insert(QueryEntry::InProgress);
            }
        }

        let value = compute(self);
        match cache(self).entries.entry(key) {
            Entry::Occupied(mut entry) => {
                debug_assert!(
                    matches!(entry.get(), QueryEntry::InProgress),
                    "the key was locked above and nothing else unlocks it"
                );
                entry.insert(QueryEntry::Done(value.clone()));
            }
            Entry::Vacant(_) => unreachable!("the key was locked above"),
        }
        Ok(value)
    }

    /// The declared name of the sort that [DefId] `def` resolves to, whether a
    /// user sort (looked up in `spec`) or a system-internal one such as
    /// `@NatPair` (looked up in `system`), or `None` when it is out of range of
    /// both.
    ///
    /// This is the single place aware that a system-internal `DefId` continues
    /// the user sort numbering: it indexes `system.sort_declarations` offset by
    /// the user sort count, the layout `resolve_system_signature` establishes.
    /// The names are derived from the specifications on demand rather than
    /// cached, so nothing here needs to stay in sync with them.
    pub(crate) fn sort_name<'a>(
        &'a self,
        spec: &'a UntypedDataSpecification,
        system: &'a UntypedDataSpecification,
        def: DefId,
    ) -> Option<&'a str> {
        if let Some(decl) = spec.sort_declarations.get(*def) {
            return Some(&decl.identifier);
        }

        let system_index = (*def).checked_sub(spec.sort_declarations.len())?;
        system
            .sort_declarations
            .get(system_index)
            .map(|decl| decl.identifier.as_str())
    }

    /// As [`Self::sort_name`], but falls back to a synthesized `@sort_N` placeholder instead of
    /// `None` when `def` is out of range.
    pub(crate) fn sort_display_name<'a>(
        &'a self,
        spec: &'a UntypedDataSpecification,
        system: &'a UntypedDataSpecification,
        def: DefId,
    ) -> Cow<'a, str> {
        match self.sort_name(spec, system, def) {
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

/// The error returned when a query transitively depends on itself.
///
/// Queries detect cycles through the cache lock state, so a cyclic definition
/// (for example a sort alias that refers to itself) surfaces as this error
/// instead of unbounded recursion.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
#[error("cyclic query dependency")]
pub(crate) struct CyclicQuery;

/// A memoization table for a single query, populated through
/// [TypeCheckContext::get_or_compute].
///
/// A query is looked up by key; a miss locks the key (marking it
/// `InProgress`) before computing its value, so a query that transitively
/// depends on itself re-enters a locked key and fails with [CyclicQuery]
/// instead of recursing unboundedly.
///
/// A locked key must always resolve to [QueryEntry::Done], so fallible
/// queries must store their failure as part of the value (`V = Result<T, E>`)
/// rather than returning early; otherwise the key stays locked and later
/// lookups misreport the failure as a [CyclicQuery].
pub(crate) struct QueryCache<K, V> {
    entries: HashMap<K, QueryEntry<V>>,
}

enum QueryEntry<V> {
    InProgress,
    Done(V),
}

impl<K: Eq + Hash, V> QueryCache<K, V> {
    pub(crate) fn new() -> Self {
        QueryCache {
            entries: HashMap::new(),
        }
    }

    /// Returns the cached value for `key` if it has already been computed,
    /// or `None` if it is not yet in the cache (or still in progress).
    /// Use this for read-only access after the pipeline has populated the cache.
    pub(crate) fn get(&self, key: &K) -> Option<&V> {
        match self.entries.get(key)? {
            QueryEntry::Done(v) => Some(v),
            QueryEntry::InProgress => None,
        }
    }

    /// Iterates the values of every entry that has finished computing. Used
    /// for read-only sweeps over the whole cache after the pipeline has run,
    /// rather than looking up one key at a time.
    pub(crate) fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.values().filter_map(|entry| match entry {
            QueryEntry::Done(value) => Some(value),
            QueryEntry::InProgress => None,
        })
    }

    /// Unconditionally stores `value` as the done result for `key`.
    ///
    /// For a caller that already has the value in hand and only needs the cache as storage.
    /// Unlike [`TypeCheckContext::get_or_compute`], this does not detect cyclic self-dependency,
    /// so it requires a query that cannot recurse into itself.
    pub(crate) fn insert(&mut self, key: K, value: V) {
        self.entries.insert(key, QueryEntry::Done(value));
    }
}

impl<K: Eq + Hash, V> Default for QueryCache<K, V> {
    fn default() -> Self {
        QueryCache::new()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use merc_syntax::DefId;

    use crate::CyclicQuery;
    use crate::ResolvedSortId;
    use crate::TypeCheckContext;

    #[test]
    fn test_get_or_compute_memoizes() {
        let mut ctx = TypeCheckContext::new();
        let key = DefId::new(1);
        let calls = Cell::new(0);

        let compute = |_: &mut TypeCheckContext| {
            calls.set(calls.get() + 1);
            ResolvedSortId::new(7)
        };
        let first = ctx
            .get_or_compute(|ctx| &mut ctx.sort_of_def, key, compute)
            .expect("no cyclic dependency");
        let second = ctx
            .get_or_compute(|ctx| &mut ctx.sort_of_def, key, compute)
            .expect("no cyclic dependency");

        assert_eq!(first, ResolvedSortId::new(7));
        assert_eq!(second, ResolvedSortId::new(7));
        assert_eq!(
            calls.get(),
            1,
            "the second lookup must hit the cache instead of recomputing"
        );
    }

    #[test]
    fn test_get_or_compute_detects_cycle() {
        let mut ctx = TypeCheckContext::new();
        let key = DefId::new(1);
        let mut inner_result = None;

        ctx.get_or_compute(
            |ctx| &mut ctx.sort_of_def,
            key,
            |ctx| {
                inner_result = Some(ctx.get_or_compute(|ctx| &mut ctx.sort_of_def, key, |_| ResolvedSortId::new(0)));
                ResolvedSortId::new(1)
            },
        )
        .expect("the outer query itself does not depend on itself");

        assert_eq!(
            inner_result,
            Some(Err(CyclicQuery)),
            "re-entering the same key from within its own computation is a cycle"
        );
    }
}
