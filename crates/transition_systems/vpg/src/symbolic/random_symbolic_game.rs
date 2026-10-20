use std::collections::BTreeMap;

use rand::Rng;
use rand::RngExt;

use oxidd::ManagerRef;
use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;
use oxidd::ldd::RelationProductMeta;
use oxidd::ldd::Value;

use merc_symbolic::SylvanTransitionGroup;
use merc_symbolic::from_iter;
use merc_utilities::MercError;

use crate::PG;
use crate::Player;
use crate::SymbolicParityGame;
use crate::VertexIndex;

/// Encodes an arbitrary explicit parity game symbolically, for use as an oracle
/// in round-trip and solver cross-check tests.
///
/// # Details
///
/// Every vertex `v` is encoded as its base-`radix` digits over `k =
/// ceil(log_radix(n))` positions; `radix` should be drawn such that `k > 1` and
/// the resulting LDD actually has several levels.
///
/// Edges are partitioned at random into `num_groups` transition relations *per
/// owner* , each reading and writing *every* position (`0..k`): a short
/// relation that only reads a subset of positions is a projection, which adds
/// spurious edges for any two states that agree on the read positions.
///
/// Returns the symbolic game, the LDD of all vertices, and the vertex index →
/// cube mapping (so a test can look up an explicit vertex's encoding).
pub fn encode_parity_game<G: PG, R: Rng>(
    manager: &LDDManagerRef,
    game: &G,
    rng: &mut R,
    radix: Value,
    num_groups: usize,
    compute_strategy: bool,
) -> Result<(SymbolicParityGame, LDDFunction, Vec<Vec<Value>>), MercError> {
    assert!(radix >= 2, "radix must be at least 2");
    assert!(num_groups >= 1, "there must be at least one transition group");

    let num_of_vertices = game.num_of_vertices();
    let k = num_of_vertices_to_width(num_of_vertices, radix);

    let cubes: Vec<Vec<Value>> = (0..num_of_vertices).map(|v| digits(v, radix, k)).collect();

    let all_vertices = from_iter(manager, cubes.iter());

    let even_cubes: Vec<&Vec<Value>> = (0..num_of_vertices)
        .filter(|&v| game.owner(VertexIndex::new(v)) == Player::Even)
        .map(|v| &cubes[v])
        .collect();
    let even_vertices = from_iter(manager, even_cubes.into_iter());

    let mut priority_cubes: BTreeMap<crate::Priority, Vec<&Vec<Value>>> = BTreeMap::new();
    for (v, cube) in cubes.iter().enumerate() {
        priority_cubes
            .entry(game.priority(VertexIndex::new(v)))
            .or_default()
            .push(cube);
    }
    let priorities = priority_cubes
        .into_iter()
        .map(|(priority, block)| (priority, from_iter(manager, block.into_iter())))
        .collect();

    // Every relation reads and writes all `k` positions, so they all share the same meta.
    let read_write: Vec<Value> = (0..k as Value).collect();
    let RelationProductMeta {
        meta,
        read_positions,
        write_positions,
    } = manager.with_manager_shared(|m| LDDFunction::relation_product_meta(m, &read_write, &read_write))?;

    // Bucket `owner.to_index() * num_groups + random` keeps every owner's edges in their own
    // range of buckets, so no bucket ever mixes source vertices of different owners.
    let mut edge_buckets: Vec<Vec<Vec<Value>>> = vec![Vec::new(); 2 * num_groups];
    for v in 0..num_of_vertices {
        let owner_offset = game.owner(VertexIndex::new(v)).to_index() * num_groups;
        for edge in game.outgoing_edges(VertexIndex::new(v)) {
            let mut vector = vec![0; read_positions.len() + write_positions.len()];
            for (i, &pos) in read_positions.iter().enumerate() {
                vector[pos] = cubes[v][i];
            }
            for (i, &pos) in write_positions.iter().enumerate() {
                vector[pos] = cubes[*edge.to()][i];
            }

            let bucket = owner_offset + rng.random_range(0..num_groups);
            edge_buckets[bucket].push(vector);
        }
    }

    let groups: Vec<SylvanTransitionGroup> = edge_buckets
        .into_iter()
        .filter(|vectors| !vectors.is_empty())
        .map(|vectors| {
            let relation = from_iter(manager, vectors.iter());
            SylvanTransitionGroup::new(relation, meta.clone(), read_write.clone(), read_write.clone())
        })
        .collect();

    let odd_vertices = all_vertices.minus(&even_vertices)?;
    let symbolic_game = SymbolicParityGame::new(
        manager,
        &groups,
        [even_vertices, odd_vertices],
        priorities,
        compute_strategy,
    )?;
    Ok((symbolic_game, all_vertices, cubes))
}

/// Returns the smallest `k` such that `radix^k >= num_of_vertices` (and at least 1).
fn num_of_vertices_to_width(num_of_vertices: usize, radix: Value) -> usize {
    let mut k = 1;
    while (radix as usize).pow(k as u32) < num_of_vertices {
        k += 1;
    }
    k
}

/// Returns the base-`radix` digits of `v` over `k` positions (most significant first).
fn digits(mut v: usize, radix: Value, k: usize) -> Vec<Value> {
    let mut result = vec![0; k];
    for i in (0..k).rev() {
        result[i] = (v % radix as usize) as Value;
        v /= radix as usize;
    }
    result
}
