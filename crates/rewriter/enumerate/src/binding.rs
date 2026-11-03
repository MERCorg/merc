use std::collections::HashSet;

use ahash::RandomState;

use merc_aterm::ATermRef;
use merc_aterm::Markable;
use merc_aterm::Protected;
use merc_aterm::ProtectedWriteGuard;
use merc_aterm::SymbolRef;
use merc_aterm::Term;
use merc_aterm::Transmutable;
use merc_aterm::storage::Marker;
use merc_data::DataApplication;
use merc_data::DataExpression;
use merc_data::DataExpressionKind;
use merc_data::DataExpressionRef;
use merc_data::DataFunctionSymbolRef;
use merc_data::DataVariable;
use merc_data::DataVariableRef;
use merc_data::mark_occurrences;
use merc_sabre::RewriteEngine;
use merc_sabre::utilities::RewriteSubstitution;

use crate::arena_list::ArenaList;
use crate::arena_list::ArenaListHandle;

/// The variable bindings chosen so far along one branch of the enumeration
/// search, recorded as a linked list of `(variable, value)` nodes stored in the
/// arena. Branches share a common prefix, so extending one leaves every sibling
/// valid.
#[derive(Clone, Copy, Default)]
pub(crate) struct BindingChain(ArenaListHandle);

/// One `(variable, value)` node.
pub(crate) struct Binding<'a> {
    variable: DataVariableRef<'a>,
    value: DataExpressionRef<'a>,
}

/// A write guard onto a [`BindingArena`]'s store, obtained once per search.
pub(crate) type BindingGuard<'g, 'env> = ProtectedWriteGuard<'g, 'env, BindingStore<'g>>;

/// The protected part of a [`BindingArena`].
///
/// `closed` lives next to the chain so that its entries are marked during
/// garbage collection and cleared together with the chain, and therefore
/// remain valid across [`BindingArena::resolve`] calls.
pub(crate) struct BindingStore<'a> {
    list: ArenaList<Binding<'a>>,
    /// Subterms of bound values known to contain no variable.
    closed: HashSet<ATermRef<'a>, RandomState>,
}

/// Backing store for every [`BindingChain`] produced during one search.
///
/// # Details
///
/// The chain is a triangular substitution: a bound variable's value may
/// mention variables bound *later* in the same chain (e.g. `v ↦ c(y1, y2)`
/// where `y1`/`y2` are fresh variables introduced to expand `v`). Use
/// [`BindingArena::resolve`] to substitute those away.
pub(crate) struct BindingArena {
    pub(crate) chain: Protected<BindingStore<'static>>,
    /// Arguments of the application being rebuilt by [`BindingArena::resolve`].
    pub(crate) arguments: Vec<DataExpression>,
}

impl Default for BindingArena {
    fn default() -> Self {
        Self::new()
    }
}

impl BindingArena {
    pub(crate) fn new() -> Self {
        BindingArena {
            chain: Protected::new(BindingStore {
                list: ArenaList::default(),
                closed: HashSet::default(),
            }),
            arguments: Vec::new(),
        }
    }

    /// Drops every node, keeping the backing storage's capacity. Must be
    /// called before starting a new search: every [`BindingChain`] handed out
    /// before a `clear` indexes into the discarded nodes and must not be used
    /// afterwards.
    pub(crate) fn clear(&mut self) {
        self.chain.write(|store| {
            store.list.clear();
            store.closed.clear();
        });
    }

    /// Returns a new chain with `variable ↦ value` recorded in front of
    /// `parent`. `parent` remains valid (siblings share it).
    pub(crate) fn extend(
        guard: &mut BindingGuard<'_, '_>,
        parent: BindingChain,
        variable: &DataVariableRef<'_>,
        value: &DataExpressionRef<'_>,
    ) -> BindingChain {
        // SAFETY: both resulting refs are inserted into `guard`'s container
        // immediately below via `push`.
        let variable_ref = unsafe { guard.protect(variable) };
        let value_ref = unsafe { guard.protect(value) };
        BindingChain(
            guard
                .list
                .push(parent.0, Binding::new(variable_ref.into(), value_ref.into())),
        )
    }

    /// Returns the immediate image bound to `variable` along `chain`, if any.
    pub(crate) fn lookup<'g>(
        guard: &BindingGuard<'g, '_>,
        chain: BindingChain,
        variable: &DataVariableRef<'_>,
    ) -> Option<DataExpressionRef<'g>> {
        Self::lookup_in(&guard.list, chain, variable)
    }

    fn lookup_in<'g>(
        list: &ArenaList<Binding<'g>>,
        chain: BindingChain,
        variable: &DataVariableRef<'_>,
    ) -> Option<DataExpressionRef<'g>> {
        let mut current = chain.0;
        while let Some((binding, parent)) = list.pop(current) {
            if binding.variable.copy() == *variable {
                return Some(binding.value.copy());
            }

            current = parent;
        }
        None
    }

    /// Applies `chain` to every variable in `vars` until no bound variable
    /// remains, rewriting each result to normal form, and stores them in `out`
    /// in the same order as `vars`. `out` is cleared first.
    ///
    /// A variable that is not bound along `chain` is free in the searched
    /// body, such as the variable of an enclosing quantifier that a one-point
    /// equation equated a bound variable with, and is left in place.
    ///
    /// The result is rewritten once at the end rather than per reconstructed
    /// application: substituting normal forms into a normal form can still
    /// create a redex (`@succ_nat` applied to a machine-word `Nat` digit), and
    /// rewriting at every level is quadratic in the chain depth.
    ///
    /// # Panics
    ///
    /// Panics if a variable in `vars` is not bound along `chain`.
    pub(crate) fn resolve<R: RewriteEngine>(
        guard: &mut BindingGuard<'_, '_>,
        rewriter: &mut R,
        arguments: &mut Vec<DataExpression>,
        chain: BindingChain,
        vars: &[DataVariable],
        out: &mut Vec<DataExpression>,
    ) {
        out.clear();

        let BindingStore { list, closed } = &mut **guard;
        for v in vars {
            let value = Self::lookup_bound(list, chain, &v.copy());
            match Self::substitute(list, closed, arguments, chain, &value, &mut false) {
                // Values are stored in normal form, so an unchanged one is final.
                None => out.push(value.protect()),
                Some(substituted) => out.push(rewriter.rewrite(&substituted)),
            }
        }
    }

    /// Like [`Self::lookup_in`], but panics when `variable` is unbound.
    fn lookup_bound<'g>(
        list: &ArenaList<Binding<'g>>,
        chain: BindingChain,
        variable: &DataVariableRef<'_>,
    ) -> DataExpressionRef<'g> {
        Self::lookup_in(list, chain, variable)
            .unwrap_or_else(|| panic!("{variable:?} is not bound in this BindingChain"))
    }

    /// Replaces every variable in `term` by its value along `chain`,
    /// recursively, without rewriting. Returns `None` when `term` contains no
    /// variable, which avoids a separate `is_closed` traversal and rebuilding
    /// closed subterms.
    ///
    /// Generic over [`Term`] so a recursive call can take the
    /// [`ATermRef`](merc_aterm::ATermRef) from `arguments()` without
    /// `protect`ing it first.
    ///
    /// Only subterms of bound values are inserted into `closed`, which the
    /// protected store keeps alive until [`BindingArena::clear`]. A subterm
    /// with an unbound variable is not, and sets `free`: that variable may
    /// well be bound along another chain.
    fn substitute<'g, 'b, T: Term<'g, 'b>>(
        list: &ArenaList<Binding<'g>>,
        closed: &mut HashSet<ATermRef<'g>, RandomState>,
        arguments: &mut Vec<DataExpression>,
        chain: BindingChain,
        term: &'b T,
        free: &mut bool,
    ) -> Option<DataExpression> {
        match DataExpressionRef::from(term.copy()).kind() {
            DataExpressionKind::Variable(variable) => {
                let Some(value) = Self::lookup_in(list, chain, &variable) else {
                    *free = true;
                    return None;
                };
                return Some(
                    Self::substitute(list, closed, arguments, chain, &value, free).unwrap_or_else(|| value.protect()),
                );
            }
            DataExpressionKind::Application(_) => {}
            // Binders never occur here: the enumerator only ever builds
            // constructor applications and plain variables.
            _ => return None,
        }

        if closed.contains(&term.copy()) {
            return None;
        }

        // At the raw term level `arg(0)` is the head symbol and the rest are
        // the actual arguments. The unchanged arguments before the first
        // substituted one are only protected once a rebuild is certain.
        let start = arguments.len();
        let mut changed = false;
        let mut subterm_free = false;
        for (index, argument) in term.arguments().skip(1).enumerate() {
            match Self::substitute(list, closed, arguments, chain, &argument, &mut subterm_free) {
                Some(substituted) => {
                    if !changed {
                        changed = true;
                        arguments.extend(term.arguments().skip(1).take(index).map(|a| a.protect().into()));
                    }
                    arguments.push(substituted);
                }
                None if changed => arguments.push(argument.protect().into()),
                None => {}
            }
        }

        *free |= subterm_free;
        if !changed {
            if !subterm_free {
                closed.insert(term.copy());
            }
            return None;
        }

        let head: DataFunctionSymbolRef<'_> = term.arg(0).into();
        let application = DataApplication::with_args(&head, &arguments[start..]).into();
        arguments.truncate(start);
        Some(application)
    }
}

impl<'a> Binding<'a> {
    fn new(variable: DataVariableRef<'a>, value: DataExpressionRef<'a>) -> Self {
        Binding { variable, value }
    }
}

impl Markable for Binding<'_> {
    fn mark(&self, marker: &mut Marker) {
        self.variable.mark(marker);
        self.value.mark(marker);
    }

    fn contains_term(&self, term: &ATermRef<'_>) -> bool {
        self.variable.contains_term(term) || self.value.contains_term(term)
    }

    fn contains_symbol(&self, symbol: &SymbolRef<'_>) -> bool {
        self.variable.contains_symbol(symbol) || self.value.contains_symbol(symbol)
    }

    fn len(&self) -> usize {
        2
    }
}

// SAFETY: `Binding<'a>` is a `#[repr(Rust)]` struct of two lifetime-erasable
// ref handles.
unsafe impl Transmutable for Binding<'static> {
    type Target<'a> = Binding<'a>;

    unsafe fn transmute_lifetime<'a>(&self) -> &'a Self::Target<'a> {
        // SAFETY: see the impl comment above.
        unsafe { std::mem::transmute::<&Self, &'a Binding<'a>>(self) }
    }

    unsafe fn transmute_lifetime_mut<'a>(&mut self) -> &'a mut Self::Target<'a> {
        // SAFETY: see the impl comment above.
        unsafe { std::mem::transmute::<&mut Self, &'a mut Binding<'a>>(self) }
    }
}

impl Markable for BindingStore<'_> {
    fn mark(&self, marker: &mut Marker) {
        self.list.mark(marker);
        for term in &self.closed {
            term.mark(marker);
        }
    }

    fn contains_term(&self, term: &ATermRef<'_>) -> bool {
        self.list.contains_term(term) || self.closed.iter().any(|closed| closed.contains_term(term))
    }

    fn contains_symbol(&self, symbol: &SymbolRef<'_>) -> bool {
        self.list.contains_symbol(symbol) || self.closed.iter().any(|closed| closed.contains_symbol(symbol))
    }

    fn len(&self) -> usize {
        self.list.len() + self.closed.len()
    }
}

// SAFETY: `BindingStore<'a>` only holds lifetime-erasable term handles, so
// its layout does not depend on `'a`; see the `Binding` impl above.
unsafe impl Transmutable for BindingStore<'static> {
    type Target<'a> = BindingStore<'a>;

    unsafe fn transmute_lifetime<'a>(&self) -> &'a Self::Target<'a> {
        // SAFETY: see the impl comment above.
        unsafe { std::mem::transmute::<&Self, &'a BindingStore<'a>>(self) }
    }

    unsafe fn transmute_lifetime_mut<'a>(&mut self) -> &'a mut Self::Target<'a> {
        // SAFETY: see the impl comment above.
        unsafe { std::mem::transmute::<&mut Self, &'a mut BindingStore<'a>>(self) }
    }
}

/// The substitution `variable ↦ value`, for rewriting a body whose earlier
/// bindings were already substituted away, so that a miss costs one
/// comparison instead of a walk over the whole chain.
pub(crate) struct SingleSubstitution<'a> {
    variable: DataVariableRef<'a>,
    value: DataExpressionRef<'a>,
}

impl<'a> SingleSubstitution<'a> {
    pub(crate) fn new(variable: DataVariableRef<'a>, value: DataExpressionRef<'a>) -> Self {
        SingleSubstitution { variable, value }
    }
}

impl RewriteSubstitution for SingleSubstitution<'_> {
    fn get<'a>(&'a self, variable: &DataVariableRef<'_>) -> Option<DataExpressionRef<'a>> {
        (self.variable == *variable).then(|| self.value.copy())
    }

    fn mark_range_occurrences(&self, variables: &[DataVariable], occurs: &mut [bool]) {
        mark_occurrences(&self.value, variables, occurs);
    }
}

#[cfg(test)]
mod tests {
    use merc_data::DataApplication;
    use merc_data::DataExpression;
    use merc_data::DataFunctionSymbol;
    use merc_data::DataVariable;
    use merc_data::SortExpression;
    use merc_sabre::InnermostRewriter;
    use merc_sabre::RewriteSpecification;

    use super::BindingArena;
    use super::BindingChain;

    #[test]
    fn test_resolve_follows_chained_bindings() {
        // v -> c(y1, y2), y1 -> a, y2 -> b: resolving v must recursively
        // substitute y1 and y2, which are only bound *later* in the chain.
        let v = DataVariable::with_sort("v", SortExpression::nat().copy());
        let y1 = DataVariable::with_sort("y1", SortExpression::nat().copy());
        let y2 = DataVariable::with_sort("y2", SortExpression::nat().copy());
        let c = DataFunctionSymbol::with_sort("c", SortExpression::nat().copy());
        let a: DataExpression = DataFunctionSymbol::with_sort("a", SortExpression::nat().copy()).into();
        let b: DataExpression = DataFunctionSymbol::with_sort("b", SortExpression::nat().copy()).into();

        let v_value: DataExpression = DataApplication::with_args(
            &c,
            &[DataExpression::from(y1.clone()), DataExpression::from(y2.clone())],
        )
        .into();

        let mut arena = BindingArena::default();
        let mut rewriter = InnermostRewriter::new(&RewriteSpecification::new(vec![]));
        let mut resolved = Vec::new();
        arena.chain.write(|guard| {
            let chain = BindingChain::default();
            let chain = BindingArena::extend(guard, chain, &v.copy(), &v_value.copy());
            let chain = BindingArena::extend(guard, chain, &y1.copy(), &a.copy());
            let chain = BindingArena::extend(guard, chain, &y2.copy(), &b.copy());

            BindingArena::resolve(
                guard,
                &mut rewriter,
                &mut arena.arguments,
                chain,
                std::slice::from_ref(&v),
                &mut resolved,
            );
        });
        let expected: DataExpression = DataApplication::with_args(&c, &[a, b]).into();
        assert_eq!(resolved, vec![expected]);
    }

    #[test]
    fn test_resolve_leaves_an_unbound_variable_in_place() {
        // v -> c(x) with x free in the searched body, as when a one-point
        // equation `v == c(x)` mentions the variable of an enclosing quantifier.
        let v = DataVariable::with_sort("v", SortExpression::nat().copy());
        let x = DataVariable::with_sort("x", SortExpression::nat().copy());
        let c = DataFunctionSymbol::with_sort("c", SortExpression::nat().copy());
        let v_value: DataExpression = DataApplication::with_args(&c, &[DataExpression::from(x)]).into();

        let mut arena = BindingArena::default();
        let mut rewriter = InnermostRewriter::new(&RewriteSpecification::new(vec![]));
        let mut resolved = Vec::new();
        arena.chain.write(|guard| {
            let chain = BindingArena::extend(guard, BindingChain::default(), &v.copy(), &v_value.copy());
            BindingArena::resolve(
                guard,
                &mut rewriter,
                &mut arena.arguments,
                chain,
                std::slice::from_ref(&v),
                &mut resolved,
            );
        });
        assert_eq!(resolved, vec![v_value]);
    }

    #[test]
    fn test_substitute_is_linear_in_a_shared_closed_value() {
        // A one-point value is an arbitrary subterm of the body, so it can be
        // maximally shared: e_{k+1} = g(e_k, e_k) has k + 1 distinct subterms
        // but unfolds to a tree of 2^k nodes, which must not be walked.
        //
        // Calls `substitute` rather than `resolve`, since the final rewrite
        // still traverses the tree.
        let v = DataVariable::with_sort("v", SortExpression::nat().copy());
        let y = DataVariable::with_sort("y", SortExpression::nat().copy());
        let c = DataFunctionSymbol::with_sort("c", SortExpression::nat().copy());
        let g = DataFunctionSymbol::with_sort("g", SortExpression::nat().copy());
        let a: DataExpression = DataFunctionSymbol::with_sort("a", SortExpression::nat().copy()).into();

        let mut shared = a.clone();
        for _ in 0..64 {
            shared = DataApplication::with_args(&g, &[shared.clone(), shared]).into();
        }
        let v_value: DataExpression =
            DataApplication::with_args(&c, &[DataExpression::from(y.clone()), shared.clone()]).into();

        let mut arena = BindingArena::default();
        let substituted = arena.chain.write(|guard| {
            let chain = BindingChain::default();
            let chain = BindingArena::extend(guard, chain, &v.copy(), &v_value.copy());
            let chain = BindingArena::extend(guard, chain, &y.copy(), &a.copy());

            let super::BindingStore { list, closed } = &mut **guard;
            BindingArena::substitute(list, closed, &mut arena.arguments, chain, &v_value, &mut false)
        });
        let expected: DataExpression = DataApplication::with_args(&c, &[a, shared]).into();
        assert_eq!(substituted, Some(expected));
    }

    #[test]
    #[should_panic(expected = "is not bound")]
    fn test_resolve_panics_on_unbound_variable() {
        let v = DataVariable::with_sort("v", SortExpression::nat().copy());
        let mut resolved = Vec::new();
        let mut arena = BindingArena::default();
        arena.chain.write(|guard| {
            BindingArena::resolve(
                guard,
                &mut InnermostRewriter::new(&RewriteSpecification::new(vec![])),
                &mut arena.arguments,
                BindingChain::default(),
                &[v],
                &mut resolved,
            )
        });
    }
}
