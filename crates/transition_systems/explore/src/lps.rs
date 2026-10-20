use std::hash::Hash;

use merc_utilities::MercError;

use crate::Slot;

/// A Linear Process Specification trait.
///
/// An [`LPS`] describes a transition system implicitly by giving an initial
/// state vector and a collection of condition action effect summands. The same
/// abstraction also covers similar state-space generators, such as PBESs in SRF
/// form.
pub trait LPS {
    /// The type of the values stored at each position of a state vector.
    ///
    /// The value must be a [`Slot`] so state vectors can be stored compactly in
    /// the discovered set's hash-consed sequence forest.
    type Value: Slot;

    /// The label produced for each enumerated transition.
    ///
    /// `Eq + Hash` lets symbolic exploration ([`crate`] consumers such as
    /// `merc_symbolic`) intern the observed labels into a dense LDD dimension.
    type Label: Clone + Eq + Hash;

    /// Metadata reported once per discovered state to the exploration caller.
    ///
    /// Plain LPSs typically use `()`.
    type StateInfo;

    /// Whether transitions carry a meaningful action label.
    const HAS_LABELS: bool = true;

    /// A single condition action effect summand of the LPS.
    type Summand: Summand<Value = Self::Value, Label = Self::Label>;

    /// Returns the initial state vector of the LPS.
    fn initial_state(&self) -> Vec<Self::Value>;

    /// Returns the summands that together define the transition relation.
    fn summands(&self) -> &[Self::Summand];

    /// Creates a fresh per-thread enumeration context for driving the summands.
    ///
    /// Each worker thread owns exactly one context; it holds the enumeration
    /// backend and the reusable scratch buffers. The LPS definition itself stays
    /// immutable and shareable by `&self`, so a single LPS can drive several
    /// contexts concurrently. The returned context is threaded through both
    /// [`LPS::prepare`] and [`Summand::enumerate`].
    fn create_context(&self) -> <Self::Summand as Summand>::Context;

    /// Prepares `context` for enumerating transitions from `state` and returns
    /// the indices of the summands that must be explored from `state`. Can be
    /// reused across all summands.
    ///
    /// The returned iterator lets implementations restrict enumeration to the
    /// relevant summands; implementations with no such restriction return all
    /// summand indices. The returned iterator may borrow both `self` and
    /// `state`, so it must be fully consumed before `state` is mutated again.
    fn prepare<'a>(
        &'a self,
        context: &mut <Self::Summand as Summand>::Context,
        state: &'a [Self::Value],
    ) -> impl Iterator<Item = usize> + 'a;

    /// Returns the state-level metadata for the given source `state`.
    ///
    /// `context` is the already-prepared enumeration context for this state;
    /// implementations that derive metadata from rewriting (e.g. general PBES)
    /// store their result in the context during [`LPS::prepare`] and read it here.
    fn state_info(&self, state: &[Self::Value], context: &<Self::Summand as Summand>::Context) -> Self::StateInfo;
}

impl<P: LPS> LPS for &P {
    type Value = P::Value;
    type Label = P::Label;
    type StateInfo = P::StateInfo;
    type Summand = P::Summand;

    const HAS_LABELS: bool = P::HAS_LABELS;

    fn initial_state(&self) -> Vec<Self::Value> {
        (**self).initial_state()
    }

    fn summands(&self) -> &[Self::Summand] {
        (**self).summands()
    }

    fn create_context(&self) -> <Self::Summand as Summand>::Context {
        (**self).create_context()
    }

    fn prepare<'a>(
        &'a self,
        context: &mut <Self::Summand as Summand>::Context,
        state: &'a [Self::Value],
    ) -> impl Iterator<Item = usize> + 'a {
        (**self).prepare(context, state)
    }

    fn state_info(&self, state: &[Self::Value], context: &<Self::Summand as Summand>::Context) -> Self::StateInfo {
        (**self).state_info(state, context)
    }
}

/// A condition action effect summand of an [`LPS`].
///
/// A summand represents a guarded transition: given a current state, it
/// reports each outgoing transition (action label and next state vector) it
/// produces by invoking the `report` callback. Implementations are free to
/// short-circuit when the callback returns an error.
pub trait Summand {
    /// The state vector element type, matching [`LPS::Value`].
    type Value;

    /// The action label type, matching [`LPS::Label`].
    type Label;

    /// Enumeration mutates this context instead of any shared `&self` state, so
    /// the same `&Summand` can be driven from several threads at once as long
    /// as each thread owns a distinct context.
    ///
    /// Contexts are produced by [`LPS::create_context`] rather than constructed
    /// directly, so a context can own backend state that depends on the LPS
    /// definition.
    type Context;

    /// Enumerate every outgoing transition produced by this summand from the
    /// state vector `state`.
    ///
    /// For each transition, `report(label, next_state)` is invoked exactly once
    /// with borrowed values.
    fn enumerate<F>(&self, context: &mut Self::Context, state: &[Self::Value], report: F) -> Result<(), MercError>
    where
        F: FnMut(&Self::Label, &[Self::Value]) -> Result<(), MercError>;

    /// Returns the indices into the state vector whose values fully determine
    /// this summand's enumeration result. Used as the cache
    /// key by [`crate::CacheLPS`].
    ///
    /// This is a *correctness* contract under caching, not a hint: when the
    /// summand is cached, two source states that agree on these positions are
    /// assumed to enumerate identical results, so any position that influences
    /// the guard or the written values must be listed.
    fn read_positions(&self) -> &[usize];

    /// Describes how this summand's next states relate to its source state.
    ///
    /// This is a *correctness* contract, not a hint: [`crate::CacheLPS`] replays
    /// cached transitions according to it, so a summand that claims
    /// [`StateEffect::Positions`] while violating it yields wrong next states on a
    /// cache hit.
    fn effect(&self) -> StateEffect<'_>;
}

/// How a [`Summand`]'s next states relate to the state they were enumerated from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateEffect<'a> {
    /// Every next state has the same length as the source state and agrees with
    /// it at every position *outside* this set.
    ///
    /// Consumers may therefore store just these positions and scatter them onto a
    /// live source state, which is what makes enumeration caching and symbolic
    /// transition relations possible.
    Positions(&'a [usize]),

    /// The next states cannot be described positionally: they may differ from the
    /// source state in length, or a position may not mean the same thing in both.
    ///
    /// A parity game generated from a PBES is the motivating case, where a single
    /// summand can emit a full parameter vector, a length-1 sink, or an auxiliary
    /// vertex. Consumers that require a positional effect (symbolic exploration)
    /// must reject this variant rather than guess.
    Opaque,
}

impl StateEffect<'_> {
    /// Returns the written positions, or `None` for [`StateEffect::Opaque`].
    pub fn positions(&self) -> Option<&[usize]> {
        match self {
            StateEffect::Positions(positions) => Some(positions),
            StateEffect::Opaque => None,
        }
    }

    /// Copies this effect into an owned value.
    pub fn to_owned(self) -> OwnedStateEffect {
        match self {
            StateEffect::Positions(positions) => OwnedStateEffect::Positions(positions.to_vec()),
            StateEffect::Opaque => OwnedStateEffect::Opaque,
        }
    }
}

/// Owned counterpart of [`StateEffect`], for implementors and wrappers that store
/// their effect rather than borrowing it from somewhere else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnedStateEffect {
    /// See [`StateEffect::Positions`].
    Positions(Vec<usize>),
    /// See [`StateEffect::Opaque`].
    Opaque,
}

impl OwnedStateEffect {
    /// Borrows this effect for returning from [`Summand::effect`].
    pub fn borrow(&self) -> StateEffect<'_> {
        match self {
            OwnedStateEffect::Positions(positions) => StateEffect::Positions(positions),
            OwnedStateEffect::Opaque => StateEffect::Opaque,
        }
    }
}
