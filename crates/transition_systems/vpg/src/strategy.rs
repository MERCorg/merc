use std::collections::HashMap;
use std::fmt;

use crate::PG;
use crate::Player;
use crate::Set;
use crate::VertexIndex;

/// A strategy abstraction used by Zielonka.
///
/// This allows running the solver with either full strategy construction
/// (`Strategy`) or with a zero-cost no-op strategy (`()`).
pub trait Strat: Sized {
    /// Creates a new empty strategy value.
    fn new() -> Self;

    /// Records a strategy move from `from` to `to`.
    fn set(&mut self, from: VertexIndex, to: VertexIndex);

    /// Gets the strategy move for `from`, if one is defined.
    fn get(&self, from: VertexIndex) -> Option<VertexIndex>;

    /// Removes any strategy move for `from`.
    fn remove(&mut self, from: VertexIndex);

    /// Combines two strategy values.
    fn union(self, other: Self) -> Self;

    /// Extends with arbitrary choices on unresolved owned vertices.
    fn extend_arbitrary<G: PG>(self, pg: &G, vertices: &Set, subgame: &Set, player: Player) -> Self;
}

/// Keeps track of a strategy for a player in a parity game.
///
/// # Details
///
/// A strategy is a partial function from vertices owned by a player to one of
/// their successors.
pub struct Strategy {
    mapping: HashMap<VertexIndex, VertexIndex>,
}

impl Strategy {
    /// Creates a new, empty strategy.
    pub fn new() -> Self {
        Self {
            mapping: HashMap::new(),
        }
    }

    /// Adds a mapping from `from` to `to` in the strategy.
    pub fn set(&mut self, from: VertexIndex, to: VertexIndex) {
        self.mapping.insert(from, to);
    }

    /// Gets the target vertex for the given source vertex, if it is defined.
    pub fn get(&self, from: VertexIndex) -> Option<&VertexIndex> {
        self.mapping.get(&from)
    }

    /// Removes the strategy entry for the given vertex, if it exists.
    pub fn remove(&mut self, from: VertexIndex) {
        self.mapping.remove(&from);
    }

    /// Computes the union of two strategies, assumes that they do not overlap.
    pub fn union(mut self, other: Strategy) -> Strategy {
        // Add all mappings from the extension strategy to the base strategy
        for (&from, &to) in other.iter() {
            debug_assert!(
                !self.mapping.contains_key(&from),
                "Cannot combine strategies with overlapping domains"
            );
            self.set(from, to);
        }

        self
    }

    /// Extends the strategy with an arbitrary strategy for the given `player` on the given `vertices`.
    pub fn extend_arbitrary<G: PG>(mut self, pg: &G, vertices: &Set, subgame: &Set, player: Player) -> Strategy {
        for vertex in vertices.iter_ones().map(VertexIndex::new) {
            if pg.owner(vertex) == player && self.get(vertex).is_none() {
                // Add an arbitrary mapping for this vertex, we can just take the first outgoing edge.
                if let Some(edge) = pg.outgoing_edges(vertex).find(|edge| subgame[*edge.to()]) {
                    self.set(vertex, edge.to());
                }
            }
        }

        self
    }

    /// Returns an iterator over all (from, to) pairs in the strategy.
    pub fn iter(&self) -> impl Iterator<Item = (&VertexIndex, &VertexIndex)> {
        self.mapping.iter()
    }

    /// Checks that the strategy is only defined for the vertices owned by the
    /// given player. Furthermore, ensure that the strategy is defined for all
    /// vertices won in the solution for the given player
    pub fn check_consistent<G: PG>(&self, pg: &G, solution: &Set, player: Player) {
        for vertex in pg.iter_vertices() {
            if solution[*vertex] && pg.owner(vertex) == player && self.get(vertex).is_none() {
                panic!(
                    "Strategy is not defined for vertex {:?} owned by {:?}, but is in the winning set.",
                    vertex,
                    pg.owner(vertex),
                );
            }

            if self.get(vertex).is_some() && pg.owner(vertex) != player {
                panic!(
                    "Strategy is defined for vertex {:?} owned by {:?}, but it should be owned by {:?}.",
                    vertex,
                    pg.owner(vertex),
                    player
                );
            }
        }
    }
}

impl Strat for Strategy {
    fn new() -> Self {
        Self::new()
    }

    fn set(&mut self, from: VertexIndex, to: VertexIndex) {
        Strategy::set(self, from, to);
    }

    fn get(&self, from: VertexIndex) -> Option<VertexIndex> {
        Strategy::get(self, from).copied()
    }

    fn remove(&mut self, from: VertexIndex) {
        Strategy::remove(self, from);
    }

    fn union(self, other: Self) -> Self {
        Strategy::union(self, other)
    }

    fn extend_arbitrary<G: PG>(self, pg: &G, vertices: &Set, subgame: &Set, player: Player) -> Self {
        Strategy::extend_arbitrary(self, pg, vertices, subgame, player)
    }
}

impl Strat for () {
    fn new() -> Self {}

    fn set(&mut self, _from: VertexIndex, _to: VertexIndex) {}

    fn get(&self, _from: VertexIndex) -> Option<VertexIndex> {
        None
    }

    fn remove(&mut self, _from: VertexIndex) {}

    fn union(self, _other: Self) -> Self {}

    fn extend_arbitrary<G: PG>(self, _pg: &G, _vertices: &Set, _subgame: &Set, _player: Player) -> Self {}
}

impl Default for Strategy {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Strategy {{")?;
        for (from, to) in &self.mapping {
            writeln!(f, "  v{} -> v{}", from, to)?;
        }
        writeln!(f, "}}")
    }
}
