use std::fmt::Debug;

use bitvec::bitvec;
use bitvec::order::Lsb0;
use bitvec::vec::BitVec;
use delegate::delegate;
use itertools::Itertools;
use log::trace;

use merc_collections::BlockIndex;
use merc_collections::BlockPartition;
use merc_collections::scc_decomposition_iterative;
use merc_utilities::MercIndex;

use crate::AsGraph;
use crate::Edge;
use crate::PG;
use crate::Player;
use crate::Pred;
use crate::Predecessors;
use crate::Priority;
use crate::Set;
use crate::VertexIndex;

/// Solves a solitaire game for the given player.
///
/// # Details
///
/// We assume that the input game is a trivial solitaire game, i.e., all
/// vertices are owned by the same player, and that the game is total. This
/// could be weakened to allow the other player to only do trivial moves, but
/// that is not yet necessary for our use case.
///
/// Solving is done by considering all subgames Gi restricted to priority `i`
/// belonging to `player`, and solving the simple solitaire game on each of
/// these subgames.
pub fn solve_solitaire_game<G: PG>(pg: &G) -> BitVec {
    debug_assert!(
        pg.iter_vertices().all(|vertex| pg.owner(vertex) == Player::Even)
            || pg.iter_vertices().all(|vertex| pg.owner(vertex) == Player::Odd),
        "solve_solitair_game requires a solitaire game"
    );

    let mut winning_vertices = bitvec![usize, Lsb0; 0; pg.num_of_vertices()];

    // The game is solitaire, so all vertices are owned by the same player.
    let player = pg.owner(pg.initial_vertex());
    let predecessors = Predecessors::new(pg);

    for priority in 0..=pg.highest_priority().value() {
        if Player::from_priority(Priority::new(priority)) != player {
            // Skip priorities that do not belong to the current player
            continue;
        }

        // Restrict the game to the current priority.
        trace!("Solving subgame for max-priority {}", priority);
        let prio_subgame = PrioSubgame::with_predecessors(pg, Priority::new(priority), &predecessors);

        let subgame_solution = solve_solitaire_simple(&prio_subgame, player);
        for vertex in prio_subgame.iter_vertices() {
            if subgame_solution[*vertex] {
                winning_vertices.set(*vertex, true)
            }
        }
    }

    backward_reachability(&predecessors, winning_vertices)
}

/// Solves a solitaire game that only contains two priorities, where `player`
/// should be the player that makes decisions.
///
/// # Details
///
/// For a solitaire game with only two priorities, the winning set for the player
/// is exactly those vertices that can reach a strongly connected component that
/// contains the highest priority. Note that the highest priority should belong
/// to the player.
fn solve_solitaire_simple<G: PG + Pred>(pg: &G, player: Player) -> BitVec {
    trace!("Subgame {{ {:?} }}, player {}", pg.iter_vertices().format(", "), player);

    let scc_partition = scc_decomposition_iterative(&AsGraph(pg), |_, _, _| true);

    // Determine vertices that are winning for the player in the restricted game, which are those that can reach a vertex with the current priority.
    let mut winning_vertices = bitvec![usize, Lsb0; 0; pg.num_of_vertices()];

    // Convert to block partition to compute reachability on the SCCs
    let block_partition = BlockPartition::<()>::from_indexed_partition(&scc_partition);
    for scc in (0..scc_partition.num_of_blocks()).map(BlockIndex::new) {
        if is_trivial_scc(pg, &block_partition, scc) {
            trace!("SCC {} is trivial, skipping", scc);
            continue;
        }

        if block_partition
            .iter_block(scc)
            // TODO: This assumes that this is the highest priority, so priorities (0,1) for odd and (1,2) for even.
            .any(|i| Player::from_priority(pg.priority(VertexIndex::new(i))) == player)
        {
            for vertex in block_partition.iter_block(scc) {
                winning_vertices.set(vertex, true);
                trace!("Player {} wins {} in SCC {}", player, vertex, scc);
            }
        }
    }

    backward_reachability(pg, winning_vertices)
}

/// Returns true if the given SCC is trivial, i.e., it does not contain any
/// cycles. This is the case if the SCC contains only one vertex and that vertex
/// does not have a self-loop.
fn is_trivial_scc<G: PG, T: Clone + Debug + Default>(pg: &G, partition: &BlockPartition<T>, block: BlockIndex) -> bool {
    if partition.iter_block(block).count() != 1 {
        // Contains at least 2 vertices, so non trivial.
        return false;
    }

    // Otherwise, must have a self loop.
    let vertex = VertexIndex::new(
        partition
            .iter_block(block)
            .next()
            .expect("Block must contain at least one vertex"),
    );
    !pg.outgoing_edges(vertex).any(|edge| edge.to() == vertex)
}

/// Computes the set of vertices reachable from the given initial vertices in
/// the given graph.
fn backward_reachability<P: Pred>(predecessors: &P, mut initial: BitVec) -> BitVec {
    // The set of vertices that are already visited.
    let visited = &mut initial;
    let mut queue: Vec<VertexIndex> = Vec::new();

    for v in visited.iter_ones() {
        queue.push(VertexIndex::new(v));
    }

    while let Some(v) = queue.pop() {
        for w in predecessors.predecessors(v) {
            if !visited.get(w.index()).expect("Vertex must be in the reachable vector") {
                trace!("Reached vertex {} from vertex {}", w, v);
                visited.set(w.index(), true);
                queue.push(w);
            }
        }
    }

    initial
}

/// A subgame induced by restricting vertices to a given set.
pub struct Subgame<'a, G: PG, P: Pred> {
    /// Vertices that are part of the subgame.
    restricted: Set,

    /// The game that is being restricted.
    game: &'a G,

    /// Predecessor provider for the original game.
    predecessors: P,
}

impl<'a, G: PG, P: Pred> Subgame<'a, G, P> {
    /// Create a new subgame induced by the given restricted-vertex set.
    pub fn new(game: &'a G, restricted: Set, predecessors: P) -> Subgame<'a, G, P> {
        debug_assert_eq!(
            restricted.len(),
            game.num_of_vertices(),
            "Restricted set must have one bit per vertex"
        );

        Subgame {
            restricted,
            game,
            predecessors,
        }
    }
}

impl<G: PG, P: Pred> PG for Subgame<'_, G, P> {
    type Label = G::Label;

    fn iter_vertices(&self) -> impl Iterator<Item = VertexIndex> + '_ {
        self.restricted.iter_ones().map(VertexIndex::new)
    }

    fn outgoing_edges<'a>(&'a self, vertex_index: VertexIndex) -> impl Iterator<Item = Edge<'a, G::Label>> + 'a {
        debug_assert!(
            self.restricted
                .get(vertex_index.value())
                .expect("Vertex must be in the restricted set"),
            "Vertex {vertex_index} is not in the restricted set"
        );

        self.game.outgoing_edges(vertex_index).filter(move |edge| {
            *self
                .restricted
                .get(*edge.to())
                .expect("Vertex must be in the restricted set")
        })
    }

    fn highest_priority(&self) -> Priority {
        self.iter_vertices()
            .fold(Priority::new(0), |max, p| max.max(self.game.priority(p)))
    }

    fn is_total(&self) -> bool {
        self.iter_vertices().all(|v| self.outgoing_edges(v).next().is_some())
    }

    delegate! {
        to self.game {
            fn initial_vertex(&self) -> VertexIndex;
            fn num_of_vertices(&self) -> usize;
            fn num_of_edges(&self) -> usize;
            fn owner(&self, vertex: VertexIndex) -> Player;
            fn priority(&self, vertex: VertexIndex) -> Priority;
        }
    }
}

impl<G: PG, P: Pred> Pred for Subgame<'_, G, P> {
    fn predecessors(&self, vertex_index: VertexIndex) -> impl Iterator<Item = VertexIndex> + '_ {
        debug_assert!(
            self.restricted
                .get(vertex_index.value())
                .expect("Vertex must be in the restricted set"),
            "Vertex must be in the restricted set"
        );

        self.predecessors.predecessors(vertex_index).filter(move |from| {
            *self
                .restricted
                .get(from.value())
                .expect("Vertex must be in the restricted set")
        })
    }
}

/// A subgame Gi induced by mapping all priorities equal to `max_priority` to
/// the player's priority, and all other priorities to the opponent's priority.
pub struct PrioSubgame<'a, G: PG, P: Pred = Predecessors<'a>> {
    /// The restricted subgame.
    subgame: Subgame<'a, G, P>,

    /// The maximum priority in the sub-game.
    max_priority: Priority,
}

impl<'a, G: PG> PrioSubgame<'a, G, Predecessors<'a>> {
    /// Create a new sub-game induced by the given strategy on the given game.
    #[cfg(test)]
    pub(crate) fn new(game: &'a G, max_priority: Priority) -> PrioSubgame<'a, G, Predecessors<'a>> {
        PrioSubgame::with_predecessors(game, max_priority, Predecessors::new(game))
    }
}

impl<'a, G: PG, P: Pred> PrioSubgame<'a, G, P> {
    /// Create a new sub-game induced by the given strategy on the given game,
    /// using the given predecessor provider.
    pub fn with_predecessors(game: &'a G, max_priority: Priority, predecessors: P) -> PrioSubgame<'a, G, P> {
        let mut restricted = bitvec![usize, Lsb0; 0; game.num_of_vertices()];
        for vertex in game.iter_vertices() {
            if game.priority(vertex) <= max_priority {
                restricted.set(vertex.value(), true);
            }
        }

        PrioSubgame {
            subgame: Subgame::new(game, restricted, predecessors),
            max_priority,
        }
    }
}

impl<G: PG, P: Pred> PG for PrioSubgame<'_, G, P> {
    type Label = G::Label;

    fn priority(&self, vertex: VertexIndex) -> Priority {
        debug_assert!(
            self.subgame
                .restricted
                .get(vertex.value())
                .expect("Vertex must be in the restricted set"),
            "Vertex must be in the restricted set"
        );

        if self.subgame.game.priority(vertex) == self.max_priority {
            // Return the priority for the player (the highest priority belongs to them).
            if self.max_priority.is_multiple_of(2) {
                Priority::new(2)
            } else {
                Priority::new(1)
            }
        } else {
            // Return the priority for the opponent.
            if self.max_priority.is_multiple_of(2) {
                Priority::new(1)
            } else {
                Priority::new(0)
            }
        }
    }

    delegate! {
        to self.subgame {
            fn iter_vertices(&self) -> impl Iterator<Item = VertexIndex> + '_;
            fn outgoing_edges<'a>(&'a self, vertex_index: VertexIndex) -> impl Iterator<Item = Edge<'a, G::Label>> + 'a;
            fn highest_priority(&self) -> Priority;
            fn is_total(&self) -> bool;
            fn initial_vertex(&self) -> VertexIndex;
            fn num_of_vertices(&self) -> usize;
            fn num_of_edges(&self) -> usize;
            fn owner(&self, vertex: VertexIndex) -> Player;
        }
    }
}

impl<G: PG, P: Pred> Pred for PrioSubgame<'_, G, P> {
    fn predecessors(&self, vertex_index: VertexIndex) -> impl Iterator<Item = VertexIndex> + '_ {
        self.subgame.predecessors(vertex_index)
    }
}

#[cfg(test)]
mod tests {
    use delegate::delegate;

    use merc_io::DumpFiles;
    use merc_utilities::random_test;

    use crate::Edge;
    use crate::PG;
    use crate::Player;
    use crate::Priority;
    use crate::VertexIndex;
    use crate::random_parity_game;
    use crate::solve_solitaire_game;
    use crate::solve_zielonka;
    use crate::write_pg;

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_solitaire_game() {
        random_test(100, |rng| {
            let files = DumpFiles::new("test_random_solitaire_game");
            let pg = random_parity_game(rng, true, 100, 3, 3);
            let solitaire = SolitaireGame::new(&pg, Player::Even);
            files.dump("input.pg", |writer| write_pg(writer, &solitaire)).unwrap();

            let solution = solve_solitaire_game(&solitaire);
            let (expected_solution, _expected_strategy) = solve_zielonka(&solitaire, false);

            assert_eq!(
                solution, expected_solution[0],
                "The solution for the solitaire solver should match the zielonka solution"
            );
        })
    }

    /// A parity game where every player is now owned by given player, making this a
    /// solitaire game.
    struct SolitaireGame<'a, G: PG> {
        game: &'a G,

        player: Player,
    }

    impl<G: PG> SolitaireGame<'_, G> {
        /// Create a new solitaire game induced by the given strategy on the given game.
        fn new<'a>(game: &'a G, player: Player) -> SolitaireGame<'a, G> {
            SolitaireGame { game, player }
        }
    }

    impl<G: PG> PG for SolitaireGame<'_, G> {
        type Label = G::Label;

        fn owner(&self, _vertex: VertexIndex) -> Player {
            // All vertices are owned by the given player, making it a solitaire game.
            self.player
        }

        delegate! {
            to self.game {
                fn initial_vertex(&self) -> VertexIndex;
                fn num_of_vertices(&self) -> usize;
                fn num_of_edges(&self) -> usize;
                fn iter_vertices(&self) -> impl Iterator<Item = VertexIndex> + '_;
                fn outgoing_edges<'a>(&'a self, vertex_index: VertexIndex) -> impl Iterator<Item = Edge<'a, G::Label>> + 'a;
                fn priority(&self, vertex: VertexIndex) -> Priority;
                fn is_total(&self) -> bool;
                fn highest_priority(&self) -> Priority;
            }
        }
    }
}
