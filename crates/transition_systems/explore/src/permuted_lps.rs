use std::sync::Arc;

use itertools::Either;

use merc_utilities::MercError;

use crate::LPS;
use crate::OwnedStateEffect;
use crate::StateEffect;
use crate::Summand;

/// A wrapper around an [`LPS`] that permutes the positions of its state vector.
///
/// The wrapper presents position `order[i]` of the wrapped LPS as position `i` of its own state
/// vectors. Everything it exposes is expressed in that permuted space: the initial state, the
/// read positions and state effect of every summand, and the enumerated next states. Consumers of
/// the wrapper therefore need no knowledge of the permutation at all, which is what turns a
/// variable order into a preprocessing step of symbolic exploration rather than a translation that
/// its transition relations have to carry.
///
/// Permuting is not free: every state handed to the wrapped LPS, and every next state it reports,
/// is copied through a scratch buffer. The identity permutation is therefore recognised once, in
/// [`PermutedLps::new`], and passes every state vector through untouched.
pub struct PermutedLps<P: LPS> {
    /// The wrapped LPS, shared with every summand wrapper.
    inner: Arc<P>,

    /// `order[i]` is the position of the inner state vector stored at position `i`, i.e. the
    /// permutation that turns an inner state vector into one of this LPS.
    order: Arc<[usize]>,

    /// The inverse of [`Self::order`]: `level_of[position]` is the position of this LPS that holds
    /// position `position` of the inner one.
    level_of: Arc<[usize]>,

    /// One wrapper per summand of the inner LPS, in the same order.
    summands: Vec<PermutedSummand<P>>,

    /// Whether [Self::order] is the identity, in which case no state vector is permuted.
    identity: bool,
}

/// A summand of a [`PermutedLps`], reporting the transitions of the summand it wraps over permuted
/// state vectors.
pub struct PermutedSummand<P: LPS> {
    /// The wrapped LPS, shared with [`PermutedLps`].
    inner: Arc<P>,

    /// Index of the wrapped summand in `inner.summands()`.
    index: usize,

    /// The read positions of the wrapped summand, mapped into permuted positions and sorted.
    read_positions: Vec<usize>,

    /// The state effect of the wrapped summand, with its positions mapped in the same way.
    effect: OwnedStateEffect,

    /// See [`PermutedLps::order`].
    order: Arc<[usize]>,

    /// See [`PermutedLps::level_of`].
    level_of: Arc<[usize]>,

    /// See [`PermutedLps::identity`].
    identity: bool,
}

/// Per-thread scratch buffers for driving a [`PermutedLps`].
pub struct PermutedContext<P: LPS> {
    /// The context of the wrapped LPS.
    inner: <P::Summand as Summand>::Context,

    /// The source state, in the positions of the wrapped LPS.
    source: Vec<P::Value>,

    /// A next state of the wrapped LPS, in the permuted positions.
    next_state: Vec<P::Value>,
}

impl<P: LPS> PermutedLps<P> {
    /// Wraps `inner` such that position `order[i]` of its state vectors becomes position `i`.
    ///
    /// Fails when `order` is not a permutation of the positions of the inner state vector, or when
    /// a summand reads or writes a position that does not exist. A summand whose effect is
    /// [`StateEffect::Opaque`] can only be wrapped by the identity permutation, since its next
    /// states have no fixed positional shape to permute.
    pub fn new(inner: P, order: Vec<usize>) -> Result<Self, MercError> {
        let inner = Arc::new(inner);
        let num_positions = inner.initial_state().len();
        validate_permutation(&order, num_positions)?;

        let identity = order.iter().enumerate().all(|(index, &position)| index == position);
        let level_of: Arc<[usize]> = inverse_permutation(&order).into();
        let order: Arc<[usize]> = order.into();

        let summands = inner
            .summands()
            .iter()
            .enumerate()
            .map(|(index, summand)| {
                let read_positions = permute_positions(summand.read_positions(), &level_of)
                    .map_err(|error| MercError::from(format!("summand {index}: {error}")))?;

                let effect = match summand.effect() {
                    StateEffect::Positions(positions) => OwnedStateEffect::Positions(
                        permute_positions(positions, &level_of)
                            .map_err(|error| MercError::from(format!("summand {index}: {error}")))?,
                    ),
                    StateEffect::Opaque if identity => OwnedStateEffect::Opaque,
                    StateEffect::Opaque => {
                        return Err(MercError::from(format!(
                            "summand {index} has an opaque state effect, whose next states cannot be permuted"
                        )));
                    }
                };

                Ok(PermutedSummand {
                    inner: Arc::clone(&inner),
                    index,
                    read_positions,
                    effect,
                    order: Arc::clone(&order),
                    level_of: Arc::clone(&level_of),
                    identity,
                })
            })
            .collect::<Result<Vec<_>, MercError>>()?;

        Ok(PermutedLps {
            inner,
            order,
            level_of,
            summands,
            identity,
        })
    }

    /// Returns the wrapped LPS, so callers can reach capabilities that this wrapper forwards rather
    /// than implements.
    pub fn inner(&self) -> &P {
        &self.inner
    }

    /// Returns the permutation this LPS applies, see [`PermutedLps::order`].
    pub fn order(&self) -> &[usize] {
        &self.order
    }
}

impl<P: LPS> LPS for PermutedLps<P> {
    type Value = P::Value;
    type Label = P::Label;
    type StateInfo = P::StateInfo;
    type Summand = PermutedSummand<P>;

    const HAS_LABELS: bool = P::HAS_LABELS;

    fn initial_state(&self) -> Vec<Self::Value> {
        let initial = self.inner.initial_state();
        if self.identity {
            return initial;
        }

        self.order.iter().map(|&position| initial[position]).collect()
    }

    fn summands(&self) -> &[Self::Summand] {
        &self.summands
    }

    fn create_context(&self) -> PermutedContext<P> {
        PermutedContext {
            inner: self.inner.create_context(),
            source: Vec::with_capacity(self.order.len()),
            next_state: Vec::with_capacity(self.order.len()),
        }
    }

    fn prepare<'a>(
        &'a self,
        context: &mut PermutedContext<P>,
        state: &'a [Self::Value],
    ) -> impl Iterator<Item = usize> + 'a {
        if self.identity {
            return Either::Left(self.inner.prepare(&mut context.inner, state));
        }

        // The returned iterator may borrow `self` and `state`, but not the context that holds the
        // un-permuted state, so the summand indices are collected before that borrow ends.
        let PermutedContext { inner, source, .. } = context;
        gather_into(source, state, &self.level_of);
        let indices: Vec<usize> = self.inner.prepare(inner, source).collect();
        Either::Right(indices.into_iter())
    }

    /// The metadata is the wrapped LPS's own, computed from the un-permuted state; a `StateInfo`
    /// that refers to state vector positions therefore refers to the inner ones.
    fn state_info(&self, state: &[Self::Value], context: &PermutedContext<P>) -> Self::StateInfo {
        if self.identity {
            return self.inner.state_info(state, &context.inner);
        }

        let source: Vec<Self::Value> = self.level_of.iter().map(|&index| state[index]).collect();
        self.inner.state_info(&source, &context.inner)
    }
}

impl<P: LPS> Summand for PermutedSummand<P> {
    type Value = P::Value;
    type Label = P::Label;
    type Context = PermutedContext<P>;

    fn enumerate<F>(&self, context: &mut Self::Context, state: &[Self::Value], mut report: F) -> Result<(), MercError>
    where
        F: FnMut(&Self::Label, &[Self::Value]) -> Result<(), MercError>,
    {
        let summand = &self.inner.summands()[self.index];
        if self.identity {
            return summand.enumerate(&mut context.inner, state, report);
        }

        // Borrow the context as disjoint fields, so the source state stays readable by the wrapped
        // enumeration while its next states are permuted into `next_state`.
        let PermutedContext {
            inner,
            source,
            next_state,
        } = context;
        gather_into(source, state, &self.level_of);

        let order = &self.order;
        summand.enumerate(inner, source, |label, next| {
            gather_into(next_state, next, order);
            report(label, next_state)
        })
    }

    fn read_positions(&self) -> &[usize] {
        &self.read_positions
    }

    fn effect(&self) -> StateEffect<'_> {
        self.effect.borrow()
    }
}

/// Checks that `permutation` is a permutation of `0..num_positions`.
///
/// # Errors
///
/// Returns an error if a position is out of range, repeated, or missing from `permutation`.
pub fn validate_permutation(permutation: &[usize], num_positions: usize) -> Result<(), MercError> {
    let mut seen = vec![false; num_positions];

    for &position in permutation {
        let assigned = seen
            .get_mut(position)
            .ok_or_else(|| format!("position {position} does not exist, there are {num_positions} position(s)"))?;
        if *assigned {
            return Err(format!("position {position} occurs more than once in the permutation").into());
        }
        *assigned = true;
    }

    if let Some(missing) = seen.iter().position(|assigned| !assigned) {
        return Err(format!("position {missing} does not occur in the permutation").into());
    }

    Ok(())
}

/// Returns the inverse of `permutation`, i.e. the index at which every position occurs in it.
///
/// # Panics
///
/// Panics if `permutation` is not a permutation of `0..permutation.len()`.
pub fn inverse_permutation(permutation: &[usize]) -> Vec<usize> {
    let mut inverse = vec![0; permutation.len()];
    for (index, &position) in permutation.iter().enumerate() {
        inverse[position] = index;
    }
    inverse
}

/// Fills `buffer` with `values[indices[i]]` for every index `i` of `indices`.
fn gather_into<V: Copy>(buffer: &mut Vec<V>, values: &[V], indices: &[usize]) {
    buffer.clear();
    buffer.extend(indices.iter().map(|&index| values[index]));
}

/// Maps `positions` of the inner state vector onto the permuted positions given by `level_of`,
/// keeping them sorted as the [`Summand`] contract expects.
fn permute_positions(positions: &[usize], level_of: &[usize]) -> Result<Vec<usize>, MercError> {
    let mut result = positions
        .iter()
        .map(|&position| {
            level_of.get(position).copied().ok_or_else(|| {
                MercError::from(format!(
                    "position {position} does not exist, there are {} position(s)",
                    level_of.len()
                ))
            })
        })
        .collect::<Result<Vec<usize>, MercError>>()?;
    result.sort_unstable();
    Ok(result)
}
