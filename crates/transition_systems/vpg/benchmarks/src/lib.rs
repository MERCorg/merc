#![forbid(unsafe_code)]

//! Shared helpers for benchmarking [`merc_vpg::SymbolicParityGame::attractor`] against
//! [`merc_vpg::SymbolicParityGame::attractor_naive`]: the incremental `todo`-frontier attractor
//! versus the naive one that recomputes control predecessors of the whole set every round instead
//! of only its newest layer (see both functions' doc comments for why they agree on the result).
//! `crates/vpg/tests/random_symbolic_game_test.rs` checks that agreement; these benchmarks only
//! compare how much work each spends getting there.

use rand::rngs::StdRng;

use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;
use oxidd::ldd::Value;

use merc_utilities::MercError;
use merc_vpg::AttractorProgress;
use merc_vpg::Player;
use merc_vpg::SymbolicParityGame;
use merc_vpg::encode_parity_game;
use merc_vpg::random_parity_game;

/// A synthetic symbolic parity game together with the seed (`alpha`, `u`) an attractor
/// benchmark runs against: the vertices at the game's own highest priority, so the attractor
/// actually grows over several iterations instead of converging immediately.
pub struct AttractorCase {
    pub game: SymbolicParityGame,
    pub alpha: Player,
    pub u: LDDFunction,
}

/// A progress tracker that never actually prints (interval far longer than any benchmark run) —
/// [`merc_vpg::SymbolicParityGame::attractor`]/`attractor_naive` require one, but a benchmark has
/// nothing useful for it to say.
pub fn silent_attractor_progress() -> AttractorProgress {
    AttractorProgress::new(|_| {}, 3600)
}

/// Builds a random explicit parity game (`num_vertices` vertices, `num_priorities` priorities, up
/// to `out_degree` outgoing edges per vertex, always total) and encodes it symbolically with a
/// `radix`-ary state vector split over `num_groups` transition relations per owner — see
/// [`merc_vpg::random_parity_game`] and [`merc_vpg::encode_parity_game`] for the generators this
/// wraps.
///
/// Never asks for a strategy: these benchmarks are about the attractor *set* computation, and
/// computing a strategy alongside it would fold a second, unrelated cost into the comparison.
pub fn generate_attractor_case(
    manager: &LDDManagerRef,
    rng: &mut StdRng,
    num_vertices: usize,
    num_priorities: usize,
    out_degree: usize,
    radix: Value,
    num_groups: usize,
) -> Result<AttractorCase, MercError> {
    let explicit = random_parity_game(rng, true, num_vertices, num_priorities, out_degree);
    let (game, all_vertices, _cubes) = encode_parity_game(manager, &explicit, rng, radix, num_groups, false)?;

    let (priority, u) = game
        .max_priority(&all_vertices)?
        .expect("a non-empty game has at least one priority");
    let alpha = Player::from_priority(priority);

    Ok(AttractorCase { game, alpha, u })
}
