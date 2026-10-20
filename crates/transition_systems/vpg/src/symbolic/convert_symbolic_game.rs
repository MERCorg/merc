use oxidd::ManagerRef;
use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;
use oxidd::ldd::Value;
use rustc_hash::FxBuildHasher;
use streaming_iterator::StreamingIterator;

use merc_collections::IndexedSet;
use merc_symbolic::element_of;
use merc_symbolic::iter;
use merc_utilities::MercError;

use crate::ParityGame;
use crate::ParityGameBuilder;
use crate::Player;
use crate::SymbolicParityGame;
use crate::VertexIndex;

/// Converts a symbolic parity game, restricted to `vertices`, into an explicit
/// [`ParityGame`].
///
/// Returns the explicit game together with the `vertex index → cube` mapping,
/// so a caller (or a test) can translate back to the original symbolic
/// vertices.
pub fn convert_symbolic_parity_game(
    manager: &LDDManagerRef,
    game: &SymbolicParityGame,
    vertices: &LDDFunction,
) -> Result<(ParityGame, Vec<Vec<Value>>), MercError> {
    let mut index: IndexedSet<Vec<Value>, FxBuildHasher> = IndexedSet::new();
    let mut cube_iter = iter(vertices);
    while let Some(cube) = cube_iter.next() {
        index.insert(cube.clone());
    }

    let mut builder = ParityGameBuilder::with_capacity(VertexIndex::new(0), index.len());

    for (set_index, cube) in index.iter() {
        let vertex = VertexIndex::new(*set_index);

        let owner = if element_of(manager, cube, game.vertices_owned_by(Player::Even)) {
            Player::Even
        } else {
            Player::Odd
        };

        let priority = game
            .priorities()
            .iter()
            .find(|(_, block)| element_of(manager, cube, block))
            .map(|(&priority, _)| priority)
            .ok_or("convert_symbolic_parity_game: vertex has no priority")?;

        builder.add_vertex(vertex, owner, priority);

        let singleton = manager.with_manager_shared(|m| LDDFunction::singleton(m, cube))?;
        let successors = game.successors(&singleton)?.intersect(vertices)?;

        let mut successor_iter = iter(&successors);
        while let Some(successor) = successor_iter.next() {
            let target = index
                .index(successor)
                .ok_or("convert_symbolic_parity_game: successor is not part of the vertex set")?;
            builder.add_edge(vertex, VertexIndex::new(*target));
        }
    }

    let cubes = index.to_vec();
    Ok((builder.finish(false, false), cubes))
}
