use bitvec::bitvec;
use bitvec::order::Lsb0;
use delegate::delegate;
use log::trace;

use crate::Edge;
use crate::PG;
use crate::Player;
use crate::Priority;
use crate::Set;
use crate::Strategy;
use crate::VertexIndex;
use crate::check_partition;
use crate::solve_solitaire_game;

/// Verifies that a proposed solution is valid for the given parity game and
/// strategies.
///
/// # Details
///
/// This is done by restricting the `pg` according to the strategy for each
/// player, and computing the solution for the induced solitaire game.
pub fn verify_solution<G: PG>(pg: &G, solution: &[Set; 2], strategy: &[Strategy; 2]) {
    debug_assert!(pg.is_total(), "Verifying requires a total parity game");

    trace!("Solution for even: {}", solution[Player::Even.to_index()]);
    trace!("Solution for odd: {}", solution[Player::Odd.to_index()]);

    trace!("Strategy for even: {:?}", strategy[Player::Even.to_index()]);
    trace!("Strategy for odd: {:?}", strategy[Player::Odd.to_index()]);

    // The set of all vertices in the game.
    let vertices = bitvec![usize, Lsb0; 1; pg.num_of_vertices()];

    // Check that the input solution is a proper partitioning
    check_partition(&solution[0], &solution[1], &vertices);

    // We check both players' strategies
    for player in [Player::Even, Player::Odd] {
        // Check that the strategies are consistent with the ownership of the vertices
        strategy[player.to_index()].check_consistent(pg, &solution[player.to_index()], player);

        // Restricted game according to the strategy for the current player
        let restricted = Restricted::new(pg, player, &strategy[player.to_index()]);

        // The opponent is the solitaire player, so invert the solution.
        // Furthermore, only consider the vertices that are in the solution for
        // the current player.
        let expected = !solve_solitaire_game(&restricted) & solution[player.to_index()].clone();
        if expected != solution[player.to_index()] {
            panic!(
                "The strategy for player {} is incorrect, expected solution {}, got {}",
                player,
                expected,
                solution[player.to_index()]
            );
        }
    }
}

/// A subgame of a parity game that is induced by taking the strategy for the
/// given player into account.
struct Restricted<'a, G: PG> {
    /// The game that is being restricted.
    game: &'a G,

    /// The strategy that is applied to the game.
    strategy: &'a Strategy,

    /// The player that owns the strategy.
    player: Player,
}

impl<G: PG> Restricted<'_, G> {
    /// Create a new sub-game induced by the given strategy on the given game.
    pub(crate) fn new<'a>(game: &'a G, player: Player, strategy: &'a Strategy) -> Restricted<'a, G> {
        Restricted { game, player, strategy }
    }
}

impl<G: PG> PG for Restricted<'_, G> {
    type Label = G::Label;

    fn owner(&self, _vertex_index: VertexIndex) -> Player {
        // We know that player only makes moves according to the strategy, so we can treat all vertices in the game as if they were owned by the opponent.
        self.player.opponent()
    }

    fn outgoing_edges<'a>(&'a self, vertex_index: VertexIndex) -> impl Iterator<Item = Edge<'a, G::Label>> + 'a {
        self.game.outgoing_edges(vertex_index).filter(move |edge| {
            // Only consider edges that follow the strategy.
            self.game.owner(vertex_index) != self.player || self.strategy.get(vertex_index) == Some(&edge.to())
        })
    }

    fn is_total(&self) -> bool {
        // The sub-game is total if every vertex in the restricted set has at least one outgoing edge.
        self.iter_vertices().all(|v| self.outgoing_edges(v).next().is_some())
    }

    delegate! {
        to self.game {
            fn initial_vertex(&self) -> VertexIndex;
            fn num_of_vertices(&self) -> usize;
            fn num_of_edges(&self) -> usize;
            fn iter_vertices(&self) -> impl Iterator<Item = VertexIndex> + '_;
            fn priority(&self, vertex: VertexIndex) -> Priority;
            fn highest_priority(&self) -> Priority;
        }
    }
}
