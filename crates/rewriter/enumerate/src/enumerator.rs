use std::cell::RefCell;
use std::collections::VecDeque;
use std::ops::ControlFlow;
use std::rc::Rc;

use ahash::HashMap;
use ahash::HashMapExt;
use merc_aterm::Protected;
use merc_data::DataApplication;
use merc_data::DataExpression;
use merc_data::DataExpressionRef;
use merc_data::DataVariable;
use merc_data::DataVariableRef;
use merc_sabre::RewriteEngine;

use crate::binding::BindingArena;
use crate::binding::BindingChain;
use crate::binding::SingleSubstitution;
use crate::enumeration_plan::EnumerationPlanId;
use crate::enumeration_plan::EnumerationPlans;
use crate::enumeration_plan::NotEnumerableReason;
use crate::enumeration_plan::SortEnumerability;
use crate::fresh::FreshVariableGenerator;
use crate::remaining::RemainingArena;
use crate::remaining::RemainingList;

/// Configures how far [`Enumerator::find_witness`] searches before giving up. A search over
/// finite sorts only always ends, so these limits do not apply to it.
///
/// Must stay fixed for the lifetime of a cache built over this enumerator's
/// results: a summand or quantifier cache keyed on read positions assumes two
/// calls with the same inputs enumerate the same solutions, a search bound that
/// changes between calls violates this assumption.
#[derive(Clone, Copy, Debug)]
pub struct EnumerationLimits {
    /// Maximum number of work items processed before giving up.
    pub max_items: usize,
    /// Maximum constructor-nesting depth per variable before giving up on
    /// that branch.
    pub max_depth: u32,
}

impl Default for EnumerationLimits {
    fn default() -> Self {
        EnumerationLimits {
            max_items: 1000,
            max_depth: 64,
        }
    }
}

/// Why an [`Enumerator::enumerate`] call stopped.
///
/// `B` is the consumer callback's [`ControlFlow::Break`] payload (`()` if it
/// never breaks with one), so a caller can thread e.g. a `Result` straight
/// out of [`Outcome::Stopped`] instead of latching it in a captured variable.
#[derive(Debug)]
pub enum Outcome<B = ()> {
    /// Every solution was reported; the search space is exhausted.
    Exhausted,
    /// The consumer callback returned [`ControlFlow::Break`], carrying its payload.
    Stopped(B),
    /// [`EnumerationLimits::max_items`] or `max_depth` was hit somewhere in
    /// the search; results are incomplete.
    LimitReached,
    /// A fully ground branch's body rewrote to neither `true` nor `false`
    /// (carried here), so it decided nothing.
    Undecided(DataExpression),
    /// `variable`'s sort cannot be enumerated at all (`reason`), so the whole
    /// search is abandoned.
    NotEnumerable(DataVariable, NotEnumerableReason),
}

/// The result of [`Enumerator::find_witness`].
#[derive(Debug)]
pub enum WitnessOutcome {
    /// A witness was found; `values` are the bound variables' values, in the
    /// same order as the `vars` argument.
    Found(Vec<DataExpression>),
    /// The search was exhausted and no witness exists.
    NoneExists,
    /// The search hit a limit.
    LimitReached,
}

/// Which quantifier [`Enumerator::find_witness`] is deciding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuantifierKind {
    /// Find a witness making the body `true`.
    Exists,
    /// Find a counterexample making the body `false`.
    Forall,
}

/// A ground solution reported by [`Enumerator::enumerate`]: the bound
/// variables' values, in the same order as the `vars` argument.
///
/// Borrows the enumerator's scratch buffer, so it is only valid for the
/// duration of the consumer callback it is handed to.
pub struct Solution<'s> {
    values: &'s [DataExpression],
}

impl Solution<'_> {
    /// Returns the bound variables' values, in `vars` order.
    pub fn values(&self) -> &[DataExpression] {
        self.values
    }
}

/// The result of [`Enumerator::find_witness_normalized`].
pub(crate) struct WitnessSearch {
    pub(crate) outcome: WitnessOutcome,
    /// The bodies of the undecided leaves, when the search was otherwise exhausted. Every
    /// other branch was rejected, so the quantifier is equivalent to their disjunction for
    /// `exists`, and their conjunction for `forall`.
    pub(crate) residuals: Option<Vec<DataExpression>>,
}

/// One unfinished branch of the search.
struct WorkItem {
    /// The variables still to be instantiated, in enumeration order, as a
    /// [`RemainingList`] of indices into [`EnumeratorFrame::var_pool`].
    remaining: RemainingList,
    /// the goal body already rewritten under the current bindings
    body: DataExpression,
    /// a `Copy` handle into the frame's `BindingArena`
    bindings: BindingChain,
    /// the search depth at which this work item was created
    depth: u32,
}

/// Enumerates ground constructor instances of a data sort, closing a
/// quantifier body (via [`Enumerator::find_witness`]) or a `sum`-variable
/// guard (via [`Enumerator::enumerate`]).
///
/// See [the search](https://mercorg.github.io/merc-website/developer/rewriting/enumeration/#the-search) for the algorithm.
pub struct Enumerator {
    plans: Rc<EnumerationPlans>,
    limits: EnumerationLimits,
    /// Finite sorts' materialised element list. Tied to the rewriter that
    /// produced them, so it must not outlive a change of `RewriteEngine`.
    finite_elements: RefCell<HashMap<EnumerationPlanId, Rc<[DataExpression]>>>,
    /// Shared by all nested frames.
    generator: RefCell<FreshVariableGenerator>,
    /// The `Bool` literals
    true_literal: DataExpression,
    false_literal: DataExpression,
}

/// The scratch state of a single [`Enumerator`] search, reused across
/// searches rather than reallocated per search.
///
/// A search must not reuse the frame of a search that is still running, such
/// as the outer one of a nested quantifier.
pub struct EnumeratorFrame {
    /// Backing store `WorkItem.remaining`'s indices point into for the
    /// current `drive` call.
    var_pool: Protected<Vec<DataVariableRef<'static>>>,
    solution_buf: Vec<DataExpression>,
    bindings_arena: BindingArena,
    remaining_arena: RemainingArena,
    queue: VecDeque<WorkItem>,
    /// Scratch buffers for one constructor expansion's fresh arguments and
    /// their `var_pool` indices, reused across every constructor of every
    /// [`SortEnumerability::InfiniteEnumerable`] variable in the search
    /// rather than reallocated per constructor.
    constructor_args_buf: Vec<DataExpression>,
    fresh_indices_buf: Vec<u32>,
}

impl Default for EnumeratorFrame {
    fn default() -> Self {
        EnumeratorFrame {
            var_pool: Protected::new(Vec::new()),
            solution_buf: Vec::new(),
            bindings_arena: BindingArena::default(),
            remaining_arena: RemainingArena::default(),
            queue: VecDeque::new(),
            constructor_args_buf: Vec::new(),
            fresh_indices_buf: Vec::new(),
        }
    }
}

impl Enumerator {
    /// Builds an enumerator with the default [`EnumerationLimits`], whose
    /// fresh variables are named by `generator`.
    ///
    /// # Details
    ///
    /// The `Rc` keeps `Enumerator` free of a lifetime parameter, so it can be
    /// stored as a plain field. The rewriter is borrowed per call instead,
    /// since it may differ between searches.
    pub fn new(plans: Rc<EnumerationPlans>, generator: FreshVariableGenerator) -> Self {
        Enumerator {
            plans,
            limits: EnumerationLimits::default(),
            finite_elements: RefCell::new(HashMap::new()),
            generator: RefCell::new(generator),
            true_literal: DataExpression::bool_literal(true),
            false_literal: DataExpression::bool_literal(false),
        }
    }

    /// Sets the [`EnumerationLimits`] used by [`Enumerator::find_witness`].
    ///
    /// Must be called before the first search, and not changed afterwards.
    pub fn with_limits(mut self, limits: EnumerationLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Enumerates every ground substitution `σ` of `vars` for which
    /// `rewrite(body, σ)` is the `Bool` literal `true`, reporting each one to
    /// `consume`.
    ///
    /// Runs to exhaustion or until `consume` returns [`ControlFlow::Break`];
    /// **never bounded by [`EnumerationLimits`]** — see that type's doc
    /// comment for why. Intended for `sum`-variable successor generation,
    /// where every solution is wanted and a truncated result would be an
    /// unsound state space.
    pub fn enumerate<R: RewriteEngine, F, B>(
        &self,
        frame: &mut EnumeratorFrame,
        rewriter: &mut R,
        vars: &[DataVariable],
        body: &DataExpression,
        consume: F,
    ) -> Outcome<B>
    where
        F: FnMut(&mut R, &Solution<'_>) -> ControlFlow<B>,
    {
        let body = rewriter.rewrite(body);
        self.enumerate_normalized(frame, rewriter, vars, body, consume)
    }

    /// Same as [`Self::enumerate`], but for a `body` the caller has already
    /// rewritten to normal form.
    pub fn enumerate_normalized<R: RewriteEngine, F, B>(
        &self,
        frame: &mut EnumeratorFrame,
        rewriter: &mut R,
        vars: &[DataVariable],
        body: DataExpression,
        consume: F,
    ) -> Outcome<B>
    where
        F: FnMut(&mut R, &Solution<'_>) -> ControlFlow<B>,
    {
        frame.bindings_arena.clear();
        self.drive(
            frame, rewriter, vars, body, false, // reject on `false`, accept on `true`
            false, // limits never apply
            None, consume,
        )
    }

    /// Searches for a ground substitution `σ` of `vars` that decides `kind`:
    /// for [`QuantifierKind::Exists`], one making `rewrite(body, σ)` `true`;
    /// for [`QuantifierKind::Forall`], a counterexample making it `false`.
    ///
    /// Bounded by [`EnumerationLimits`], unless every variable has a finite sort: that search
    /// always ends, so it runs to a decision.
    ///
    /// The one-point rule is not applied, since it is a preprocessing step for the caller, see
    /// [`crate::simplify_one_point`].
    pub fn find_witness<R: RewriteEngine>(
        &self,
        frame: &mut EnumeratorFrame,
        rewriter: &mut R,
        vars: &[DataVariable],
        body: &DataExpression,
        kind: QuantifierKind,
    ) -> WitnessOutcome {
        let body = rewriter.rewrite(body);
        self.find_witness_normalized(frame, rewriter, vars, body, kind).outcome
    }

    /// Same as [`Self::find_witness`], but for a `body` the caller has
    /// already rewritten to normal form.
    ///
    /// Also returns what a quantifier can be simplified to when the search reaches no
    /// decision, see [`WitnessSearch`].
    pub(crate) fn find_witness_normalized<R: RewriteEngine>(
        &self,
        frame: &mut EnumeratorFrame,
        rewriter: &mut R,
        vars: &[DataVariable],
        body: DataExpression,
        kind: QuantifierKind,
    ) -> WitnessSearch {
        // `reject_is_true` says which literal settles the quantifier for a
        // branch without deciding it; the other produces the witness.
        let reject_is_true = matches!(kind, QuantifierKind::Forall);

        frame.bindings_arena.clear();

        // Over finite sorts only, the search introduces no fresh variables and so always ends.
        let apply_limits = !vars.iter().all(|variable| self.is_finite(variable));

        let mut found: Option<Vec<DataExpression>> = None;
        let mut residuals = Vec::new();
        let outcome = self.drive(
            frame,
            rewriter,
            vars,
            body,
            reject_is_true,
            apply_limits,
            Some(&mut residuals),
            |_rewriter, solution| {
                found = Some(solution.values().to_vec());
                ControlFlow::Break(())
            },
        );

        let (outcome, residuals) = match (outcome, found) {
            (_, Some(values)) => (WitnessOutcome::Found(values), None),
            (Outcome::Exhausted, None) => (WitnessOutcome::NoneExists, None),
            (Outcome::Undecided(_), None) => (WitnessOutcome::LimitReached, Some(residuals)),
            (Outcome::LimitReached, None) | (Outcome::NotEnumerable(..), None) => (WitnessOutcome::LimitReached, None),
            (Outcome::Stopped(()), None) => {
                unreachable!("Stopped only occurs when on_leaf returns Break, which only happens once `found` is set")
            }
        };

        WitnessSearch { outcome, residuals }
    }

    /// Returns true iff the sort of `variable` can be enumerated.
    pub(crate) fn is_enumerable(&self, variable: &DataVariable) -> bool {
        self.plans
            .get(&variable.sort())
            .is_some_and(|id| !matches!(self.plans.plan(id).enumerability(), SortEnumerability::NotEnumerable(_)))
    }

    /// Returns true iff the sort of `variable` has finitely many elements.
    fn is_finite(&self, variable: &DataVariable) -> bool {
        self.plans
            .get(&variable.sort())
            .is_some_and(|id| self.plans.plan(id).enumerability() == SortEnumerability::Finite)
    }

    /// Returns `sort_id`'s materialised element list, computing and caching it
    /// on first use.
    ///
    /// Returns an owned handle, and holds no borrow of the cache while
    /// rewriting, since the caller rewrites under each element in turn.
    fn finite_elements<R: RewriteEngine>(&self, rewriter: &mut R, sort_id: EnumerationPlanId) -> Rc<[DataExpression]> {
        if let Some(elements) = self.finite_elements.borrow().get(&sort_id) {
            return elements.clone();
        }

        let elements = self.materialize_finite_elements(rewriter, sort_id);
        self.finite_elements
            .borrow_mut()
            .entry(sort_id)
            .or_insert(elements)
            .clone()
    }

    fn materialize_finite_elements<R: RewriteEngine>(
        &self,
        rewriter: &mut R,
        sort_id: EnumerationPlanId,
    ) -> Rc<[DataExpression]> {
        let mut result = Vec::new();

        for constructor in self.plans.plan(sort_id).constructors() {
            if constructor.arity() == 0 {
                // A constant is not automatically a normal form either.
                let value: DataExpression = constructor.symbol().clone().into();
                result.push(rewriter.rewrite(&value));
                continue;
            }

            let mut per_argument: Vec<Vec<DataExpression>> = Vec::with_capacity(constructor.arity());
            for &argument_sort in constructor.arguments() {
                per_argument.push(self.finite_elements(rewriter, argument_sort).to_vec());
            }

            for combination in cartesian_product(&per_argument) {
                let value: DataExpression = DataApplication::with_args(constructor.symbol(), &combination).into();
                result.push(rewriter.rewrite(&value));
            }
        }

        result.into()
    }

    /// Expands `vars` breadth-first, pruning any branch whose body
    /// has already rewritten to `reject` regardless of its still-free
    /// variables, and reports every leaf (fully ground) branch whose body is
    /// `accept` to `on_leaf`.
    ///
    /// A leaf that is neither `reject` nor `accept` decided nothing, so the
    /// search reports [`Outcome::Undecided`] rather than
    /// [`Outcome::Exhausted`]: the two are indistinguishable from inside the
    /// search, and only the caller knows whether an under-approximated result
    /// set is acceptable.
    ///
    /// `vars` is expanded in the given order; fresh variables introduced while
    /// expanding a constructor are appended at the tail of each work item's
    /// own remaining list (§4.1/§4.5).
    ///
    /// `reject_is_true` selects which of the enumerator's cached `Bool`
    /// literals is the rejecting one. The search stops at [`EnumerationLimits`]
    /// only when `apply_limits` is set. The body of every undecided leaf is
    /// pushed to `residuals`, when given.
    #[allow(clippy::too_many_arguments)]
    fn drive<R: RewriteEngine, F, B>(
        &self,
        frame: &mut EnumeratorFrame,
        rewriter: &mut R,
        vars: &[DataVariable],
        body: DataExpression,
        reject_is_true: bool,
        apply_limits: bool,
        mut residuals: Option<&mut Vec<DataExpression>>,
        mut on_leaf: F,
    ) -> Outcome<B>
    where
        F: FnMut(&mut R, &Solution<'_>) -> ControlFlow<B>,
    {
        let limits = self.limits;
        let mut processed: usize = 0;
        let mut truncated = false;
        let mut undecided: Option<DataExpression> = None;
        let (reject, accept) = if reject_is_true {
            (&self.true_literal, &self.false_literal)
        } else {
            (&self.false_literal, &self.true_literal)
        };

        frame.queue.clear();
        frame.remaining_arena.clear();
        // Both guards are held for the whole call rather than re-acquired per
        // access.
        frame.var_pool.write(|var_pool| {
            var_pool.clear();
            frame.bindings_arena.chain.write(|bindings| {
                for variable in vars {
                    // SAFETY: the resulting ref is pushed into `var_pool` immediately below.
                    let var_ref = unsafe { var_pool.protect(variable) };
                    var_pool.push(var_ref.into());
                }
                // `var_pool` was cleared above, so the original variables occupy its first indices.
                let initial_indices = 0..u32::try_from(var_pool.len()).expect("more variables than fit in a u32");

                frame.queue.push_back(WorkItem {
                    remaining: RemainingList::new(&mut frame.remaining_arena, initial_indices),
                    body,
                    bindings: BindingChain::default(),
                    depth: 0,
                });

                while let Some(item) = frame.queue.pop_front() {
                    if item.body == *reject {
                        continue;
                    }

                    // Counted here rather than per constructor expansion, so the bound
                    // covers every branch the search takes, not only the ones over an
                    // infinite sort.
                    if apply_limits {
                        if processed >= limits.max_items {
                            return Outcome::LimitReached;
                        }
                        processed += 1;
                    }

                    if item.remaining.is_empty() {
                        if item.body != *accept {
                            // `undecided` keeps one body for `Outcome::Undecided`, which tells the
                            // caller that the search was exhausted without a decision; `residuals`
                            // collects all of them, when the caller asked for that.
                            if let Some(residuals) = residuals.as_deref_mut() {
                                residuals.push(item.body.clone());
                            }
                            undecided.get_or_insert(item.body);
                            continue;
                        }

                        BindingArena::resolve(
                            bindings,
                            rewriter,
                            &mut frame.bindings_arena.arguments,
                            item.bindings,
                            vars,
                            &mut frame.solution_buf,
                        );
                        let solution = Solution {
                            values: &frame.solution_buf,
                        };

                        match on_leaf(rewriter, &solution) {
                            ControlFlow::Break(b) => return Outcome::Stopped(b),
                            ControlFlow::Continue(()) => continue,
                        }
                    }

                    // Only a constructor expansion nests deeper.
                    if apply_limits && item.depth >= limits.max_depth {
                        truncated = true;
                        continue;
                    }

                    // Not protected: `variable_ref` borrows straight from `var_pool`.
                    let (variable_index, rest) = item
                        .remaining
                        .pop_front(&mut frame.remaining_arena)
                        .expect("checked non-empty above");
                    let variable_ref = &var_pool[variable_index as usize];

                    let Some(sort_id) = self.plans.get(&variable_ref.sort()) else {
                        return Outcome::NotEnumerable(variable_ref.protect(), NotEnumerableReason::UnknownSort);
                    };

                    match self.plans.plan(sort_id).enumerability() {
                        SortEnumerability::NotEnumerable(reason) => {
                            return Outcome::NotEnumerable(variable_ref.protect(), reason);
                        }
                        SortEnumerability::Finite => {
                            // No mutation of `var_pool` happens in this arm, so
                            // `variable_ref` stays valid for the whole loop as-is.
                            let elements = self.finite_elements(rewriter, sort_id);
                            for element in elements.iter() {
                                let element_ref = element.copy();
                                let new_bindings =
                                    BindingArena::extend(bindings, item.bindings, variable_ref, &element_ref);
                                let new_body = rewrite_bound(rewriter, &item.body, variable_ref, &element_ref);
                                frame.queue.push_back(WorkItem {
                                    remaining: rest,
                                    body: new_body,
                                    bindings: new_bindings,
                                    depth: item.depth,
                                });
                            }
                        }
                        SortEnumerability::InfiniteEnumerable => {
                            for constructor in self.plans.plan(sort_id).constructors() {
                                frame.fresh_indices_buf.clear();
                                frame.constructor_args_buf.clear();
                                for &argument_sort in constructor.arguments() {
                                    let fresh = self
                                        .generator
                                        .borrow_mut()
                                        .generate(self.plans.plan(argument_sort).sort().copy());
                                    frame.constructor_args_buf.push(DataExpression::from(fresh.clone()));
                                    // SAFETY: the resulting ref is pushed into `var_pool` immediately below.
                                    let fresh_ref = unsafe { var_pool.protect(&fresh) };
                                    frame
                                        .fresh_indices_buf
                                        .push(u32::try_from(var_pool.len()).expect("more variables than fit in a u32"));
                                    var_pool.push(fresh_ref.into());
                                }

                                let value: DataExpression = if frame.constructor_args_buf.is_empty() {
                                    constructor.symbol().clone().into()
                                } else {
                                    DataApplication::with_args(constructor.symbol(), &frame.constructor_args_buf).into()
                                };
                                // `rewrite_with` splices substitution images in without rewriting them,
                                // so they must already be normal forms — a constructor application is
                                // not automatically one (`@c0`/`@succ_nat(_)` under a machine-word
                                // `Nat` encoding still rewrite into digit form).
                                let rewritten = rewriter.rewrite(&value);
                                let value_ref = rewritten.copy();

                                // Re-borrowed from `var_pool` here, rather than reusing
                                // `variable_ref` from before the match.
                                let variable_ref = &var_pool[variable_index as usize];
                                let new_bindings =
                                    BindingArena::extend(bindings, item.bindings, &variable_ref.copy(), &value_ref);
                                let new_body = rewrite_bound(rewriter, &item.body, &variable_ref.copy(), &value_ref);

                                frame.queue.push_back(WorkItem {
                                    remaining: rest
                                        .with_appended(&mut frame.remaining_arena, frame.fresh_indices_buf.drain(..)),
                                    body: new_body,
                                    bindings: new_bindings,
                                    depth: item.depth + 1,
                                });
                            }
                        }
                    }
                }

                if truncated {
                    Outcome::LimitReached
                } else if let Some(body) = undecided {
                    Outcome::Undecided(body)
                } else {
                    Outcome::Exhausted
                }
            })
        })
    }
}

/// Rewrites `body` under `variable ↦ value`.
///
/// Only the newest binding is needed: every earlier one was substituted away
/// in a prior step, so `body` no longer mentions those variables.
fn rewrite_bound<R: RewriteEngine>(
    rewriter: &mut R,
    body: &DataExpression,
    variable: &DataVariableRef<'_>,
    value: &DataExpressionRef<'_>,
) -> DataExpression {
    rewriter.rewrite_with(body, &SingleSubstitution::new(variable.copy(), value.copy()))
}

/// Returns the cartesian product of `lists`: one combination per element of
/// the result, each holding one element from every list in order. An empty
/// `lists` yields a single empty combination — the identity of the product,
/// which is what an arity-0 constructor needs.
pub(crate) fn cartesian_product(lists: &[Vec<DataExpression>]) -> Vec<Vec<DataExpression>> {
    let mut result: Vec<Vec<DataExpression>> = vec![Vec::new()];
    for list in lists {
        let mut next = Vec::with_capacity(result.len() * list.len());
        for prefix in &result {
            for element in list {
                let mut combination = prefix.clone();
                combination.push(element.clone());
                next.push(combination);
            }
        }
        result = next;
    }
    result
}
