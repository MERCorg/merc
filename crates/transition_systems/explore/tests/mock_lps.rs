#![allow(dead_code)]

use merc_explore::LPS;
use merc_explore::StateEffect;
use merc_explore::Summand;
use merc_utilities::MercError;

/// Action label produced by a transition: the label of the firing summand.
pub(crate) type Label = u32;

/// A reachable state, as a vector of coordinate values.
pub(crate) type State = Vec<usize>;

/// A single transition over state vectors, `(source, label, target)`.
pub(crate) type Edge = (State, Label, State);

/// A mock [`LPS`] over numbered state vectors of fixed dimension.
///
/// Each summand is a guarded increment: a conjunction of strict-`<` inequality
/// guards on individual coordinates and an effect that, when every guard holds,
/// adds a value to a set of coordinates.
#[derive(Clone)]
pub(crate) struct MockLps {
    initial: State,
    summands: Vec<MockSummand>,
}

impl MockLps {
    /// Builds an LPS from an initial vector and a set of summands.
    pub(crate) fn new(initial: State, summands: Vec<MockSummand>) -> MockLps {
        MockLps { initial, summands }
    }

    /// A `d`-dimensional grid: one increment-summand per coordinate `i`, guarded
    /// by the single inequality `state[i] < bounds[i]`.
    pub(crate) fn grid(bounds: &[usize]) -> MockLps {
        let summands = (0..bounds.len())
            .map(|i| MockSummand::new(i as Label, vec![(i, bounds[i])], vec![i]))
            .collect();
        MockLps::new(vec![0; bounds.len()], summands)
    }

    /// A `d`-dimensional grid whose coordinate-`i` summand, instead of a fixed
    /// `+1`, adds any value in `0..=steps[i]`, fanning out into several
    /// successors per state. The `steps` give a different `k` for every state
    /// parameter; a `steps[i]` of `0` makes that summand a guarded self-loop.
    pub(crate) fn grid_with_steps(bounds: &[usize], steps: &[usize]) -> MockLps {
        assert_eq!(bounds.len(), steps.len(), "one step bound per coordinate");
        let summands = (0..bounds.len())
            .map(|i| MockSummand::with_step(i as Label, vec![(i, bounds[i])], i, steps[i]))
            .collect();
        MockLps::new(vec![0; bounds.len()], summands)
    }

    /// A grid extended with a "diagonal" summand whose guard is a conjunction of
    /// two inequalities and whose effect increments both coordinates at once.
    /// Requires at least two dimensions.
    pub(crate) fn grid_with_diagonal(bounds: &[usize]) -> MockLps {
        assert!(bounds.len() >= 2, "diagonal needs at least two dimensions");
        let mut lps = MockLps::grid(bounds);
        lps.summands.push(MockSummand::new(
            bounds.len() as Label,
            vec![(0, bounds[0]), (1, bounds[1])],
            vec![0, 1],
        ));
        lps
    }
}

/// A guarded-increment summand of [`MockLps`].
#[derive(Clone)]
pub(crate) struct MockSummand {
    label: Label,
    /// Conjunction of guards, each requiring `state[pos] < bound`.
    guards: Vec<(usize, usize)>,
    /// Effect: per written coordinate, the inclusive `(pos, lo, hi)` interval of
    /// values added to it when every guard holds. The cartesian product of these
    /// intervals enumerates the successors.
    increments: Vec<(usize, usize, usize)>,
    read_positions: Vec<usize>,
    write_positions: Vec<usize>,
}

impl MockSummand {
    /// Creates a summand that adds one to each listed coordinate, producing a
    /// single successor per state.
    pub(crate) fn new(label: Label, guards: Vec<(usize, usize)>, increments: Vec<usize>) -> MockSummand {
        let increments = increments.into_iter().map(|pos| (pos, 1, 1)).collect();
        MockSummand::with_intervals(label, guards, increments)
    }

    /// Creates a summand that adds any value in `0..=step` to `pos`, producing
    /// `step + 1` successors per state.
    pub(crate) fn with_step(label: Label, guards: Vec<(usize, usize)>, pos: usize, step: usize) -> MockSummand {
        MockSummand::with_intervals(label, guards, vec![(pos, 0, step)])
    }

    /// Creates a summand from its guard conjunction and per-coordinate delta
    /// intervals, deriving the read/write position sets the cache contract
    /// requires.
    pub(crate) fn with_intervals(
        label: Label,
        guards: Vec<(usize, usize)>,
        increments: Vec<(usize, usize, usize)>,
    ) -> MockSummand {
        // The result depends on every guarded coordinate and, because the
        // effect is a relative increment, on every written coordinate too.
        let mut read_positions: Vec<usize> = guards
            .iter()
            .map(|&(pos, _)| pos)
            .chain(increments.iter().map(|&(pos, _, _)| pos))
            .collect();
        read_positions.sort_unstable();
        read_positions.dedup();

        let mut write_positions: Vec<usize> = increments.iter().map(|&(pos, _, _)| pos).collect();
        write_positions.sort_unstable();
        write_positions.dedup();

        MockSummand {
            label,
            guards,
            increments,
            read_positions,
            write_positions,
        }
    }

    fn fires(&self, state: &[usize]) -> bool {
        self.guards.iter().all(|&(pos, bound)| state[pos] < bound)
    }
}

/// Per-thread scratch for [`MockSummand::enumerate`]: the next-state buffer.
#[derive(Default)]
pub(crate) struct MockContext {
    next: State,
}

impl LPS for MockLps {
    type Value = usize;
    type Label = Label;
    type StateInfo = State;
    type Summand = MockSummand;

    fn initial_state(&self) -> State {
        self.initial.clone()
    }

    fn summands(&self) -> &[MockSummand] {
        &self.summands
    }

    fn create_context(&self) -> MockContext {
        MockContext::default()
    }

    fn prepare<'a>(&'a self, _context: &mut MockContext, _state: &'a [usize]) -> impl Iterator<Item = usize> + 'a {
        0..self.summands.len()
    }

    fn state_info(&self, state: &[usize], _context: &MockContext) -> State {
        // Surface the live state vector so tests can map dense indices back to
        // the concrete vectors the drivers assign them.
        state.to_vec()
    }
}

impl Summand for MockSummand {
    type Value = usize;
    type Label = Label;
    type Context = MockContext;

    fn enumerate<F>(&self, context: &mut MockContext, state: &[usize], mut report: F) -> Result<(), MercError>
    where
        F: FnMut(&Label, &[usize]) -> Result<(), MercError>,
    {
        if !self.fires(state) {
            return Ok(());
        }

        // Mixed-radix odometer over the inclusive delta intervals: each setting
        // of `deltas` is one point of the cartesian product, hence one successor.
        let mut deltas: Vec<usize> = self.increments.iter().map(|&(_, lo, _)| lo).collect();
        loop {
            context.next.clear();
            context.next.extend_from_slice(state);
            for (&(pos, _, _), &delta) in self.increments.iter().zip(&deltas) {
                context.next[pos] += delta;
            }
            report(&self.label, &context.next)?;

            // Advance the rightmost digit that has not reached its upper bound,
            // resetting the digits to its right; stop once all are saturated.
            let mut digit = self.increments.len();
            loop {
                if digit == 0 {
                    return Ok(());
                }
                digit -= 1;
                let (_, _, hi) = self.increments[digit];
                if deltas[digit] < hi {
                    deltas[digit] += 1;
                    for (slot, &(_, lo, _)) in deltas[digit + 1..].iter_mut().zip(&self.increments[digit + 1..]) {
                        *slot = lo;
                    }
                    break;
                }
            }
        }
    }

    fn read_positions(&self) -> &[usize] {
        &self.read_positions
    }

    fn effect(&self) -> StateEffect<'_> {
        StateEffect::Positions(&self.write_positions)
    }
}
