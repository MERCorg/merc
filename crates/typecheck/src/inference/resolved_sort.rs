use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;

use merc_syntax::ComplexSort;
use merc_syntax::DefId;
use merc_syntax::Sort;
use merc_syntax::TypeVarId;
use merc_syntax::UntypedDataSpecification;
use merc_utilities::TagIndex;

use crate::TypeCheckContext;

/// A unique type for interned resolved sorts.
pub(crate) struct ResolvedSortTag;

/// An index into a [SortInterner], identifying a unique resolved sort.
///
/// Because sorts are interned, two ids are equal if and only if the sorts they
/// denote are equal, so equality of sorts is a comparison of two integers.
pub(crate) type ResolvedSortId = TagIndex<usize, ResolvedSortTag>;

/// A type in the mCRL2 type system, called a *sort*.
///
/// The built-in sorts and container constructors reuse the AST enums
/// [merc_syntax::Sort] and [merc_syntax::ComplexSort]. Sub-sorts are stored as
/// [ResolvedSortId] indices into the [SortInterner] rather than by value, so a
/// `ResolvedSort` is small and structural equality coincides with id equality.
/// Using an index arena rather than reference counting is how the rest of the
/// MERC workspace models pooled data.
///
/// Unlike the book, the number sorts form a lattice (see [SortInterner::join]
/// and [SortInterner::meet]): `Pos <= Nat <= Int <= Real`, and for containers
/// `FSet(S) <= Set(S)` and `FBag(S) <= Bag(S)`. This models the implicit
/// coercions that mCRL2 inserts during type checking.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ResolvedSort {
    /// The sort with a single element, used internally for the result of an
    /// action. It has no surface syntax, which is why it is a variant here
    /// rather than a member of [merc_syntax::Sort].
    Unit,
    /// A built-in primitive sort such as `Bool` or `Nat`.
    Primitive(Sort),
    /// A container sort such as `List(S)` or `Set(S)`.
    Generic { op: ComplexSort, subsort: ResolvedSortId },
    /// A function sort `A_0 # ... # A_n -> B`.
    Function {
        domain: Vec<ResolvedSortId>,
        range: ResolvedSortId,
    },
    /// A user-defined (nominal) sort, identified by the declaration it resolves
    /// to. Two `Def` sorts are equal only when they refer to the same
    /// declaration, and otherwise incomparable.
    Def(DefId),
    /// A bound type variable, scoped to the polymorphic specification that
    /// introduces it.
    Var(TypeVarId),
}

impl ResolvedSort {
    /// Compares two sorts by the sub-sort ordering, assuming both come from the
    /// same interner (so id equality means sort equality).
    ///
    /// Number sorts are ordered by generality and container sorts by their
    /// finiteness marker when their element sorts are equal; all other distinct
    /// sorts are incomparable.
    fn partial_cmp_in(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (ResolvedSort::Primitive(lhs), ResolvedSort::Primitive(rhs)) => primitive_partial_cmp(*lhs, *rhs),
            (
                ResolvedSort::Generic {
                    op: lhs_op,
                    subsort: lhs_sub,
                },
                ResolvedSort::Generic {
                    op: rhs_op,
                    subsort: rhs_sub,
                },
            ) => {
                if lhs_sub == rhs_sub {
                    generic_op_partial_cmp(*lhs_op, *rhs_op)
                } else {
                    None
                }
            }
            _ if self == other => Some(Ordering::Equal),
            _ => None,
        }
    }
}

/// Returns the generality of a number sort (`Pos` = 0, `Nat` = 1, `Int` = 2,
/// `Real` = 3), or `None` for the non-number sorts.
pub(crate) fn number_generality(sort: Sort) -> Option<u32> {
    match sort {
        Sort::Pos => Some(0),
        Sort::Nat => Some(1),
        Sort::Int => Some(2),
        Sort::Real => Some(3),
        Sort::Bool => None,
    }
}

/// The inverse of [number_generality].
pub(crate) fn number_sort_from_generality(generality: u32) -> Sort {
    match generality {
        0 => Sort::Pos,
        1 => Sort::Nat,
        2 => Sort::Int,
        3 => Sort::Real,
        _ => panic!("{generality} is not a number sort generality"),
    }
}

/// Renders a resolved sort for debug logging. Nominal sorts take their name
/// from [TypeCheckContext::sort_name] (a user or system-internal sort such as
/// `@NatPair`), falling back to a bare index.
pub(crate) struct DisplaySortContext<'a> {
    ctx: &'a TypeCheckContext,
    spec: &'a UntypedDataSpecification,
    system: &'a UntypedDataSpecification,
    id: ResolvedSortId,
}

impl<'a> DisplaySortContext<'a> {
    pub(crate) fn new(
        ctx: &'a TypeCheckContext,
        spec: &'a UntypedDataSpecification,
        system: &'a UntypedDataSpecification,
        id: ResolvedSortId,
    ) -> Self {
        DisplaySortContext { ctx, spec, system, id }
    }

    /// A [DisplaySortContext] for a sub-sort of `self`, reusing the same context
    /// and specifications.
    fn sub(&self, id: ResolvedSortId) -> Self {
        DisplaySortContext {
            ctx: self.ctx,
            spec: self.spec,
            system: self.system,
            id,
        }
    }
}

impl fmt::Display for DisplaySortContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.ctx.sorts.get(self.id) {
            ResolvedSort::Unit => write!(f, "@Unit"),
            ResolvedSort::Primitive(sort) => write!(f, "{sort}"),
            ResolvedSort::Generic { op, subsort } => {
                write!(f, "{op}({})", self.sub(*subsort))
            }
            ResolvedSort::Function { domain, range } => {
                let domain: Vec<String> = domain.iter().map(|sort| self.sub(*sort).to_string()).collect();
                write!(f, "{} -> {}", domain.join(" # "), self.sub(*range))
            }
            ResolvedSort::Def(def) => {
                write!(f, "{}", self.ctx.sort_display_name(self.spec, self.system, *def))
            }
            // Debug logging only (per this struct's doc comment).
            ResolvedSort::Var(id) => write!(f, "@S_{id}"),
        }
    }
}

/// Orders the primitive sorts by the number-sort hierarchy. Distinct sorts that
/// are not both numbers are incomparable.
///
/// This is deliberately not [merc_syntax::Sort]'s derived ordering, which is
/// lexical by declaration order rather than by generality.
fn primitive_partial_cmp(lhs: Sort, rhs: Sort) -> Option<Ordering> {
    if lhs == rhs {
        Some(Ordering::Equal)
    } else if let (Some(lhs), Some(rhs)) = (number_generality(lhs), number_generality(rhs)) {
        lhs.partial_cmp(&rhs)
    } else {
        None
    }
}

/// Panics: neither `Unit` nor `Var` ever denotes a data-expression's own resolved sort — `Unit`
/// has no surface syntax (only an action's result uses it, see [`ResolvedSort::Unit`]'s own doc)
/// and every bound type variable is instantiated to a fresh unification variable
/// (`ConstraintGenerator::instantiate_scheme`) before Phase-3 solving ever produces a final
/// [`ResolvedSortId`] — so a renderer of an already-resolved *value* sort should never reach
/// either. Shared by the two structural recursions over [`ResolvedSort`] that only ever render a
/// value sort: `crate::typing_info::sort_expression` and `crate::ir::mcrl2_lowering::lower_sort`.
pub(crate) fn unreachable_not_a_value_sort(variant: &str) -> ! {
    unreachable!("{variant} never denotes a data-expression's own resolved sort")
}

/// Returns whether the container constructor is `Set` or `FSet`.
fn is_any_set(op: ComplexSort) -> bool {
    matches!(op, ComplexSort::Set | ComplexSort::FSet)
}

/// Returns whether the container constructor is `Bag` or `FBag`.
fn is_any_bag(op: ComplexSort) -> bool {
    matches!(op, ComplexSort::Bag | ComplexSort::FBag)
}

/// Orders the container constructors by their finiteness marker: `FSet <= Set`
/// and `FBag <= Bag`. All other distinct constructors are incomparable.
fn generic_op_partial_cmp(lhs: ComplexSort, rhs: ComplexSort) -> Option<Ordering> {
    match (lhs, rhs) {
        (lhs, rhs) if lhs == rhs => Some(Ordering::Equal),
        (ComplexSort::FBag, ComplexSort::Bag) => Some(Ordering::Less),
        (ComplexSort::Bag, ComplexSort::FBag) => Some(Ordering::Greater),
        (ComplexSort::FSet, ComplexSort::Set) => Some(Ordering::Less),
        (ComplexSort::Set, ComplexSort::FSet) => Some(Ordering::Greater),
        _ => None,
    }
}

/// Interns [ResolvedSort]s so that equality is an integer comparison and each
/// distinct sort is stored once.
///
/// The primitive sorts are interned eagerly and returned by the `*_sort`
/// accessors; every other sort is created on demand through [SortInterner::generic],
/// [SortInterner::function] and [SortInterner::def].
pub(crate) struct SortInterner {
    arena: Vec<ResolvedSort>,
    dedup: HashMap<ResolvedSort, ResolvedSortId>,

    unit_sort: ResolvedSortId,
    bool_sort: ResolvedSortId,
    pos_sort: ResolvedSortId,
    nat_sort: ResolvedSortId,
    int_sort: ResolvedSortId,
    real_sort: ResolvedSortId,
}

impl SortInterner {
    pub(crate) fn new() -> Self {
        let mut interner = SortInterner {
            arena: Vec::new(),
            dedup: HashMap::new(),
            unit_sort: ResolvedSortId::new(0),
            bool_sort: ResolvedSortId::new(0),
            pos_sort: ResolvedSortId::new(0),
            nat_sort: ResolvedSortId::new(0),
            int_sort: ResolvedSortId::new(0),
            real_sort: ResolvedSortId::new(0),
        };

        interner.unit_sort = interner.intern(ResolvedSort::Unit);
        interner.bool_sort = interner.intern(ResolvedSort::Primitive(Sort::Bool));
        interner.pos_sort = interner.intern(ResolvedSort::Primitive(Sort::Pos));
        interner.nat_sort = interner.intern(ResolvedSort::Primitive(Sort::Nat));
        interner.int_sort = interner.intern(ResolvedSort::Primitive(Sort::Int));
        interner.real_sort = interner.intern(ResolvedSort::Primitive(Sort::Real));

        interner
    }

    fn intern(&mut self, sort: ResolvedSort) -> ResolvedSortId {
        if let Some(id) = self.dedup.get(&sort) {
            return *id;
        }

        let id = ResolvedSortId::new(self.arena.len());
        self.arena.push(sort.clone());
        self.dedup.insert(sort, id);
        id
    }

    pub(crate) fn primitive(&self, sort: Sort) -> ResolvedSortId {
        match sort {
            Sort::Bool => self.bool_sort,
            Sort::Pos => self.pos_sort,
            Sort::Int => self.int_sort,
            Sort::Nat => self.nat_sort,
            Sort::Real => self.real_sort,
        }
    }

    /// Interns the container sort `op(subsort)`.
    pub(crate) fn generic(&mut self, op: ComplexSort, subsort: ResolvedSortId) -> ResolvedSortId {
        self.intern(ResolvedSort::Generic { op, subsort })
    }

    /// Interns the function sort `domain -> range`.
    pub(crate) fn function(&mut self, domain: Vec<ResolvedSortId>, range: ResolvedSortId) -> ResolvedSortId {
        self.intern(ResolvedSort::Function { domain, range })
    }

    /// Interns the nominal sort for the given declaration.
    pub(crate) fn def(&mut self, def: DefId) -> ResolvedSortId {
        self.intern(ResolvedSort::Def(def))
    }

    /// Interns the bound type variable `id`. Two calls with the same `id`
    /// return the same [ResolvedSortId], which is what makes two occurrences
    /// of the same `type_var` inside one declaration denote the same sort.
    pub(crate) fn var(&mut self, id: TypeVarId) -> ResolvedSortId {
        self.intern(ResolvedSort::Var(id))
    }
}

impl SortInterner {
    /// Returns the resolved sort denoted by an id.
    ///
    /// The id must have been produced by this same interner; ids from a
    /// different [SortInterner] index into an unrelated arena and would return a
    /// wrong sort or panic.
    pub(crate) fn get(&self, id: ResolvedSortId) -> &ResolvedSort {
        debug_assert!(
            *id < self.arena.len(),
            "id {id:?} does not originate from this interner"
        );
        &self.arena[*id]
    }

    // Renders the `Unit` sort and the `Int`/`Real` literals of an inserted
    // cast. Exercised by tests only for now.
    #[allow(dead_code)]
    pub(crate) fn unit_sort(&self) -> ResolvedSortId {
        self.unit_sort
    }

    pub(crate) fn bool_sort(&self) -> ResolvedSortId {
        self.bool_sort
    }

    pub(crate) fn pos_sort(&self) -> ResolvedSortId {
        self.pos_sort
    }

    pub(crate) fn nat_sort(&self) -> ResolvedSortId {
        self.nat_sort
    }

    #[allow(dead_code)]
    pub(crate) fn int_sort(&self) -> ResolvedSortId {
        self.int_sort
    }

    #[allow(dead_code)]
    pub(crate) fn real_sort(&self) -> ResolvedSortId {
        self.real_sort
    }

    /// Compares two sorts by the sub-sort ordering. `lowering.rs` uses this to
    /// decide which side of an application argument or equation join a
    /// coercion belongs on.
    pub(crate) fn partial_cmp(&self, lhs: ResolvedSortId, rhs: ResolvedSortId) -> Option<Ordering> {
        if lhs == rhs {
            return Some(Ordering::Equal);
        }
        self.get(lhs).partial_cmp_in(self.get(rhs))
    }

    /// Finds the least common supersort of two sorts, or `None` when they are
    /// incomparable.
    ///
    /// This operation is commutative, associative and idempotent. It does not
    /// report errors, it simply returns `None`.
    // Used by Phase-3 inference to resolve the shared free variable of a group
    // of `Sub` constraints (a `Join`; see inference.rs) in one step, and by
    // Phase-4 coercion materialization.
    pub(crate) fn join(&mut self, lhs: ResolvedSortId, rhs: ResolvedSortId) -> Option<ResolvedSortId> {
        if lhs == rhs {
            return Some(lhs);
        }

        match (self.get(lhs).clone(), self.get(rhs).clone()) {
            (ResolvedSort::Primitive(lhs), ResolvedSort::Primitive(rhs)) => {
                let lhs = number_generality(lhs)?;
                let rhs = number_generality(rhs)?;
                Some(self.primitive(number_sort_from_generality(lhs.max(rhs))))
            }
            (
                ResolvedSort::Generic {
                    op: lhs_op,
                    subsort: lhs_sub,
                },
                ResolvedSort::Generic {
                    op: rhs_op,
                    subsort: rhs_sub,
                },
            ) => {
                if lhs_sub != rhs_sub {
                    return None;
                }
                let op = if lhs_op == rhs_op {
                    lhs_op
                } else if is_any_bag(lhs_op) && is_any_bag(rhs_op) {
                    ComplexSort::Bag
                } else if is_any_set(lhs_op) && is_any_set(rhs_op) {
                    ComplexSort::Set
                } else {
                    return None;
                };
                Some(self.generic(op, lhs_sub))
            }
            _ => None,
        }
    }

    /// Finds the greatest common subsort of two sorts, or `None` when they are
    /// incomparable.
    // The dual of `join`; exercised by tests only for now.
    #[allow(dead_code)]
    pub(crate) fn meet(&mut self, lhs: ResolvedSortId, rhs: ResolvedSortId) -> Option<ResolvedSortId> {
        if lhs == rhs {
            return Some(lhs);
        }

        match (self.get(lhs).clone(), self.get(rhs).clone()) {
            (ResolvedSort::Primitive(lhs), ResolvedSort::Primitive(rhs)) => {
                let lhs = number_generality(lhs)?;
                let rhs = number_generality(rhs)?;
                Some(self.primitive(number_sort_from_generality(lhs.min(rhs))))
            }
            (
                ResolvedSort::Generic {
                    op: lhs_op,
                    subsort: lhs_sub,
                },
                ResolvedSort::Generic {
                    op: rhs_op,
                    subsort: rhs_sub,
                },
            ) => {
                if lhs_sub != rhs_sub {
                    return None;
                }
                let op = if lhs_op == rhs_op {
                    lhs_op
                } else if is_any_bag(lhs_op) && is_any_bag(rhs_op) {
                    ComplexSort::FBag
                } else if is_any_set(lhs_op) && is_any_set(rhs_op) {
                    ComplexSort::FSet
                } else {
                    return None;
                };
                Some(self.generic(op, lhs_sub))
            }
            _ => None,
        }
    }
}

impl Default for SortInterner {
    fn default() -> Self {
        SortInterner::new()
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering::Equal;
    use std::cmp::Ordering::Greater;
    use std::cmp::Ordering::Less;

    use merc_syntax::ComplexSort;
    use merc_syntax::DefId;

    use crate::ResolvedSortId;
    use crate::SortInterner;

    /// A set of example sorts covering the different sort forms.
    struct ExampleSorts {
        function_sort1: ResolvedSortId, // Pos # Nat -> Real
        function_sort2: ResolvedSortId, // Real # Real -> Pos
        function_sort3: ResolvedSortId, // Pos # Nat # Nat -> Real
        function_sort4: ResolvedSortId, // Pos # Nat -> List(Real)
        def_sort1: ResolvedSortId,
        def_sort2: ResolvedSortId,
        fbag_sort1: ResolvedSortId, // FBag(Real # Real -> Pos)
        fbag_sort2: ResolvedSortId, // FBag(Pos # Nat -> Real)
        bag_sort1: ResolvedSortId,  // Bag(Pos # Nat -> Real)
        bag_sort2: ResolvedSortId,  // Bag(Pos # Nat # Nat -> Real)
        fset_sort1: ResolvedSortId, // FSet(Int)
        set_sort1: ResolvedSortId,  // Set(Int)
        set_sort2: ResolvedSortId,  // Set(Pos # Nat -> Real)
    }

    impl ExampleSorts {
        fn new(c: &mut SortInterner) -> ExampleSorts {
            let function_sort1 = c.function(vec![c.pos_sort(), c.nat_sort()], c.real_sort());
            let function_sort2 = c.function(vec![c.real_sort(), c.real_sort()], c.pos_sort());
            let function_sort3 = c.function(vec![c.pos_sort(), c.nat_sort(), c.nat_sort()], c.real_sort());
            let real_list = c.generic(ComplexSort::List, c.real_sort());
            let function_sort4 = c.function(vec![c.pos_sort(), c.nat_sort()], real_list);

            ExampleSorts {
                function_sort1,
                function_sort2,
                function_sort3,
                function_sort4,
                def_sort1: c.def(DefId::new(0)),
                def_sort2: c.def(DefId::new(1)),
                fbag_sort1: c.generic(ComplexSort::FBag, function_sort2),
                fbag_sort2: c.generic(ComplexSort::FBag, function_sort1),
                bag_sort1: c.generic(ComplexSort::Bag, function_sort1),
                bag_sort2: c.generic(ComplexSort::Bag, function_sort3),
                fset_sort1: c.generic(ComplexSort::FSet, c.int_sort()),
                set_sort1: c.generic(ComplexSort::Set, c.int_sort()),
                set_sort2: c.generic(ComplexSort::Set, function_sort1),
            }
        }
    }

    #[test]
    fn test_partial_ord() {
        let mut c = SortInterner::new();
        let e = ExampleSorts::new(&mut c);

        assert_eq!(c.partial_cmp(c.bool_sort(), c.pos_sort()), None);
        assert_eq!(c.partial_cmp(c.bool_sort(), c.bool_sort()), Some(Equal));
        assert_eq!(c.partial_cmp(c.nat_sort(), c.real_sort()), Some(Less));
        assert_eq!(c.partial_cmp(c.real_sort(), c.nat_sort()), Some(Greater));
        assert_eq!(c.partial_cmp(c.nat_sort(), c.nat_sort()), Some(Equal));
        assert_eq!(c.partial_cmp(c.pos_sort(), c.real_sort()), Some(Less));
        assert_eq!(c.partial_cmp(c.unit_sort(), c.real_sort()), None);
        assert_eq!(c.partial_cmp(c.unit_sort(), c.unit_sort()), Some(Equal));
        assert_eq!(c.partial_cmp(c.unit_sort(), c.bool_sort()), None);
        assert_eq!(c.partial_cmp(c.real_sort(), c.real_sort()), Some(Equal));
        assert_eq!(c.partial_cmp(c.real_sort(), c.int_sort()), Some(Greater));

        assert_eq!(c.partial_cmp(e.function_sort1, e.function_sort2), None);
        assert_eq!(c.partial_cmp(e.function_sort1, e.function_sort1), Some(Equal));
        assert_eq!(c.partial_cmp(e.function_sort1, e.function_sort3), None);
        assert_eq!(c.partial_cmp(e.function_sort3, e.function_sort1), None);
        assert_eq!(c.partial_cmp(e.function_sort4, e.function_sort1), None);
        assert_eq!(c.partial_cmp(e.function_sort4, e.function_sort4), Some(Equal));

        assert_eq!(c.partial_cmp(e.bag_sort1, e.fbag_sort1), None);
        assert_eq!(c.partial_cmp(e.bag_sort1, e.fbag_sort2), Some(Greater));
        assert_eq!(c.partial_cmp(e.bag_sort1, e.bag_sort2), None);
        assert_eq!(c.partial_cmp(e.fset_sort1, e.set_sort1), Some(Less));
        assert_eq!(c.partial_cmp(e.set_sort1, e.set_sort2), None);

        assert_eq!(c.partial_cmp(e.def_sort1, e.def_sort1), Some(Equal));
        assert_eq!(c.partial_cmp(e.def_sort1, e.def_sort2), None);
        assert_eq!(c.partial_cmp(e.def_sort1, c.real_sort()), None);
        assert_eq!(c.partial_cmp(e.function_sort1, e.def_sort2), None);
    }

    #[test]
    fn test_join() {
        let mut c = SortInterner::new();
        let e = ExampleSorts::new(&mut c);

        assert_eq!(c.join(c.bool_sort(), c.pos_sort()), None);
        assert_eq!(c.join(c.pos_sort(), c.pos_sort()), Some(c.pos_sort()));
        assert_eq!(c.join(c.pos_sort(), c.nat_sort()), Some(c.nat_sort()));
        assert_eq!(c.join(c.real_sort(), c.nat_sort()), Some(c.real_sort()));

        assert_eq!(c.join(e.function_sort1, e.function_sort2), None);
        assert_eq!(c.join(e.function_sort1, e.function_sort4), None);
        assert_eq!(c.join(e.function_sort4, e.function_sort4), Some(e.function_sort4));

        assert_eq!(c.join(e.set_sort1, e.fset_sort1), Some(e.set_sort1));
        assert_eq!(c.join(e.bag_sort1, e.fbag_sort2), Some(e.bag_sort1));
        assert_eq!(c.join(e.def_sort1, e.def_sort1), Some(e.def_sort1));
        assert_eq!(c.join(e.def_sort1, e.def_sort2), None);
    }

    #[test]
    fn test_meet() {
        let mut c = SortInterner::new();
        let e = ExampleSorts::new(&mut c);

        assert_eq!(c.meet(c.bool_sort(), c.pos_sort()), None);
        assert_eq!(c.meet(c.pos_sort(), c.pos_sort()), Some(c.pos_sort()));
        assert_eq!(c.meet(c.pos_sort(), c.nat_sort()), Some(c.pos_sort()));
        assert_eq!(c.meet(c.real_sort(), c.nat_sort()), Some(c.nat_sort()));

        assert_eq!(c.meet(e.function_sort1, e.function_sort2), None);
        assert_eq!(c.meet(e.function_sort1, e.function_sort4), None);
        assert_eq!(c.meet(e.function_sort4, e.function_sort4), Some(e.function_sort4));

        assert_eq!(c.meet(e.set_sort1, e.fset_sort1), Some(e.fset_sort1));
        assert_eq!(c.meet(e.bag_sort1, e.fbag_sort2), Some(e.fbag_sort2));
        assert_eq!(c.meet(e.def_sort1, e.def_sort1), Some(e.def_sort1));
        assert_eq!(c.meet(e.def_sort1, e.def_sort2), None);
    }
}
