#![forbid(unsafe_code)]

use oxidd::BooleanFunction;
use oxidd::bdd::BDDFunction;
use oxidd::bdd::BDDManagerRef;

use merc_collections::ByteCompressedVec;
use merc_collections::CompressedEntry;
use merc_collections::DedupOutcome;
use merc_collections::dedup_by_bucket;

use crate::ParityGame;
use crate::Player;
use crate::PlayerVec;
use crate::Priority;
use crate::VariabilityParityGame;
use crate::VertexIndex;

/// A trait for building parity games incrementally.
///
/// # Details
///
/// Mirrors `merc_lts::LtsBuilder`: an exploration that needs the actual game
/// (e.g. to solve it) uses [`ParityGameBuilder`], while one that only cares
/// about side effects of exploring - timing, or the vertex/edge counts the
/// exploration loop itself logs - can use the discarding `()` implementation
/// below instead, skipping the cost of materialising the game entirely.
pub trait PGBuilder {
    /// The result type of the builder once finalized.
    type PG;

    /// Adds a vertex to the builder with its owner and priority.
    fn add_vertex(&mut self, vertex: VertexIndex, owner: Player, priority: Priority);

    /// Adds an edge to the builder.
    fn add_edge(&mut self, from: VertexIndex, to: VertexIndex);

    /// Returns the number of vertices that the builder currently found.
    fn num_of_vertices(&self) -> usize;

    /// Returns the number of edges added to the builder.
    fn num_of_edges(&self) -> usize;

    /// Finalizes the builder and returns the constructed parity game.
    fn finish(self, make_total: bool, remove_duplicates: bool) -> Self::PG;
}

impl PGBuilder for ParityGameBuilder {
    type PG = ParityGame;

    fn add_vertex(&mut self, vertex: VertexIndex, owner: Player, priority: Priority) {
        ParityGameBuilder::add_vertex(self, vertex, owner, priority);
    }

    fn add_edge(&mut self, from: VertexIndex, to: VertexIndex) {
        ParityGameBuilder::add_edge(self, from, to);
    }

    fn num_of_vertices(&self) -> usize {
        ParityGameBuilder::num_of_vertices(self)
    }

    fn num_of_edges(&self) -> usize {
        ParityGameBuilder::num_of_edges(self)
    }

    fn finish(self, make_total: bool, remove_duplicates: bool) -> Self::PG {
        ParityGameBuilder::finish(self, make_total, remove_duplicates)
    }
}

/// A builder that discards all vertices and edges, producing no output. Useful
/// when a parity game only needs to be explored (e.g. for the timing and
/// vertex/edge counts the exploration loop logs as it goes) but the resulting
/// game itself is not required; solving always needs the real
/// [`ParityGameBuilder`].
impl PGBuilder for () {
    type PG = ();

    fn add_vertex(&mut self, _vertex: VertexIndex, _owner: Player, _priority: Priority) {}

    fn add_edge(&mut self, _from: VertexIndex, _to: VertexIndex) {}

    fn num_of_vertices(&self) -> usize {
        0
    }

    fn num_of_edges(&self) -> usize {
        0
    }

    fn finish(self, _make_total: bool, _remove_duplicates: bool) -> Self::PG {}
}

/// A builder for parity games that accepts edges one by one and can remove
/// duplicates.
///
/// Edges are stored in a pair of [`ByteCompressedVec`] columns rather than a
/// `Vec<(VertexIndex, VertexIndex)>`, for the same reason `merc_lts::LtsBuilderMem`
/// does: it keeps memory usage down for large games. Deduplication buckets edges by
/// their `from` vertex and removes repeats within each bucket, which avoids ever
/// sorting or permuting those columns. The per-vertex owner and priority arrays
/// are stored in the same compact form that [`ParityGame`] itself uses, so
/// [`ParityGameBuilder::finish`] can hand them over without a conversion.
pub struct ParityGameBuilder {
    /// The source vertex of every edge.
    edges_from: ByteCompressedVec<VertexIndex>,

    /// The target vertex of every edge.
    edges_to: ByteCompressedVec<VertexIndex>,

    /// The owner of each vertex, indexed by vertex index.
    owners: PlayerVec,

    /// The priority of each vertex, indexed by vertex index.
    priorities: ByteCompressedVec<Priority>,

    /// The initial vertex of the game.
    initial_vertex: VertexIndex,

    /// The number of vertices discovered so far.
    num_of_vertices: usize,
}

impl ParityGameBuilder {
    /// Initializes a new empty builder with the given initial vertex.
    pub fn new(initial_vertex: VertexIndex) -> Self {
        Self::with_capacity(initial_vertex, 0)
    }

    /// Initializes the builder with pre-allocated capacity for edges.
    pub fn with_capacity(initial_vertex: VertexIndex, num_of_edges: usize) -> Self {
        let num_of_vertices = initial_vertex.value() + 1;
        Self {
            edges_from: ByteCompressedVec::with_capacity(num_of_edges, num_of_vertices.bytes_required()),
            edges_to: ByteCompressedVec::with_capacity(num_of_edges, num_of_vertices.bytes_required()),
            owners: PlayerVec::from_elem(Player::Even, num_of_vertices),
            priorities: ByteCompressedVec::from_elem(Priority::new(0), num_of_vertices),
            initial_vertex,
            num_of_vertices,
        }
    }

    /// Adds a vertex to the builder with its owner and priority.
    pub fn add_vertex(&mut self, vertex: VertexIndex, owner: Player, priority: Priority) {
        let num_of_vertices = vertex.value() + 1;
        self.ensure_vertex_capacity(num_of_vertices);
        self.owners.set(vertex.value(), owner);
        self.priorities.set(vertex.value(), priority);
        self.num_of_vertices = self.num_of_vertices.max(num_of_vertices);
    }

    /// Adds an edge to the builder.
    pub fn add_edge(&mut self, from: VertexIndex, to: VertexIndex) {
        self.edges_from.push(from);
        self.edges_to.push(to);
        let num_of_vertices = self.num_of_vertices.max(from.value() + 1).max(to.value() + 1);
        self.ensure_vertex_capacity(num_of_vertices);
        self.num_of_vertices = num_of_vertices;
    }

    /// Returns the number of edges added to the builder.
    pub fn num_of_edges(&self) -> usize {
        self.edges_from.len()
    }

    /// Returns the number of vertices that the builder currently found.
    pub fn num_of_vertices(&self) -> usize {
        self.num_of_vertices
    }

    /// Finalizes the builder and returns the constructed parity game.
    pub fn finish(mut self, make_total: bool, remove_duplicates: bool) -> ParityGame {
        if remove_duplicates {
            self.remove_duplicates();
        }

        self.ensure_vertex_capacity(self.num_of_vertices);

        // Destructure so the edge closure can borrow `edges_from`/`edges_to` while
        // `owners` and `priorities` are moved into `from_edges`, avoiding a copy.
        let Self {
            edges_from,
            edges_to,
            owners,
            priorities,
            initial_vertex,
            ..
        } = self;
        ParityGame::from_edges(initial_vertex, owners, priorities, make_total, || {
            edges_from.iter().zip(edges_to.iter())
        })
    }

    /// Ensures that the owners and priorities vectors have enough capacity for the given number of vertices.
    fn ensure_vertex_capacity(&mut self, num_of_vertices: usize) {
        if self.owners.len() < num_of_vertices {
            self.owners.resize(num_of_vertices, Player::Even);
        }
        if self.priorities.len() < num_of_vertices {
            self.priorities.resize_with(num_of_vertices, || Priority::new(0));
        }
    }

    /// Removes duplicate edges in place, deduplicating via
    /// [`merc_collections::dedup_by_bucket`] (see its docs for the algorithm).
    fn remove_duplicates(&mut self) {
        let num_vertices = self.num_of_vertices;
        let num_edges = self.edges_from.len();

        if num_edges == 0 {
            return;
        }

        let mut new_edges_from = ByteCompressedVec::with_capacity(num_edges, num_vertices.bytes_required());
        let mut new_edges_to = ByteCompressedVec::with_capacity(num_edges, num_vertices.bytes_required());

        dedup_by_bucket(
            num_vertices,
            num_edges,
            |i| self.edges_from.index(i).value(),
            |i| self.edges_to.index(i),
            |i, from, outcome| {
                if let DedupOutcome::Keep { .. } = outcome {
                    new_edges_from.push(VertexIndex::new(from));
                    new_edges_to.push(self.edges_to.index(i));
                }
            },
        );

        new_edges_from.shrink_to_fit();
        new_edges_to.shrink_to_fit();

        self.edges_from = new_edges_from;
        self.edges_to = new_edges_to;
    }
}

/// A builder for variability parity games that accepts edges with BDD configurations
/// one by one and can remove duplicates.
///
/// Vertex indices are stored in a pair of [`ByteCompressedVec`] columns, like
/// [`ParityGameBuilder`]; the BDD configuration per edge stays a plain `Vec`
/// since a [`BDDFunction`] handle isn't a fixed-width integer [`CompressedEntry`]
/// can compress, and deduplication needs to call its BDD-level `or` anyway.
pub struct VariabilityParityGameBuilder {
    /// The source vertex of every edge.
    edges_from: ByteCompressedVec<VertexIndex>,

    /// The configuration under which every edge is enabled.
    edges_configuration: Vec<BDDFunction>,

    /// The target vertex of every edge.
    edges_to: ByteCompressedVec<VertexIndex>,

    /// The owner of each vertex, indexed by vertex index.
    owners: PlayerVec,

    /// The priority of each vertex, indexed by vertex index.
    priorities: ByteCompressedVec<Priority>,

    /// The initial vertex of the game.
    initial_vertex: VertexIndex,

    /// The number of vertices discovered so far.
    num_of_vertices: usize,
}

impl VariabilityParityGameBuilder {
    /// Initializes a new empty builder with the given initial vertex.
    #[allow(dead_code)]
    pub fn new(initial_vertex: VertexIndex) -> Self {
        Self::with_capacity(initial_vertex, 0)
    }

    /// Initializes the builder with pre-allocated capacity for edges.
    pub fn with_capacity(initial_vertex: VertexIndex, num_of_edges: usize) -> Self {
        let num_of_vertices = initial_vertex.value() + 1;
        Self {
            edges_from: ByteCompressedVec::with_capacity(num_of_edges, num_of_vertices.bytes_required()),
            edges_configuration: Vec::with_capacity(num_of_edges),
            edges_to: ByteCompressedVec::with_capacity(num_of_edges, num_of_vertices.bytes_required()),
            owners: PlayerVec::from_elem(Player::Even, num_of_vertices),
            priorities: ByteCompressedVec::from_elem(Priority::new(0), num_of_vertices),
            initial_vertex,
            num_of_vertices,
        }
    }

    /// Adds a vertex to the builder with its owner and priority.
    pub fn add_vertex(&mut self, vertex: VertexIndex, owner: Player, priority: Priority) {
        let num_of_vertices = vertex.value() + 1;
        self.ensure_vertex_capacity(num_of_vertices);
        self.owners.set(vertex.value(), owner);
        self.priorities.set(vertex.value(), priority);
        self.num_of_vertices = self.num_of_vertices.max(num_of_vertices);
    }

    /// Adds an edge to the builder with its configuration.
    pub fn add_edge(&mut self, from: VertexIndex, configuration: oxidd::bdd::BDDFunction, to: VertexIndex) {
        self.edges_from.push(from);
        self.edges_configuration.push(configuration);
        self.edges_to.push(to);
        let num_of_vertices = self.num_of_vertices.max(from.value() + 1).max(to.value() + 1);
        self.ensure_vertex_capacity(num_of_vertices);
        self.num_of_vertices = num_of_vertices;
    }

    /// Returns the number of edges added to the builder.
    #[allow(dead_code)]
    pub fn num_of_edges(&self) -> usize {
        self.edges_from.len()
    }

    /// Returns the number of vertices that the builder currently found.
    pub fn num_of_vertices(&self) -> usize {
        self.num_of_vertices
    }

    /// Consumes the builder and returns the constructed variability parity game.
    pub fn finish(
        mut self,
        manager_ref: &BDDManagerRef,
        configuration: BDDFunction,
        variables: Vec<BDDFunction>,
        remove_duplicates: bool,
    ) -> VariabilityParityGame {
        if remove_duplicates {
            self.remove_duplicates();
        }

        self.ensure_vertex_capacity(self.num_of_vertices);

        // Destructure so the edge closure can borrow the edge columns while
        // `owners` and `priorities` are moved into `from_edges`, avoiding a copy.
        let Self {
            edges_from,
            edges_configuration,
            edges_to,
            owners,
            priorities,
            initial_vertex,
            ..
        } = self;
        VariabilityParityGame::from_edges(
            manager_ref,
            initial_vertex,
            owners,
            priorities,
            configuration,
            variables,
            || {
                edges_from
                    .iter()
                    .zip(edges_configuration.iter().cloned())
                    .zip(edges_to.iter())
                    .map(|((from, configuration), to)| (from, configuration, to))
            },
        )
    }

    /// Ensures that the owners and priorities vectors have enough capacity for the given number of vertices.
    fn ensure_vertex_capacity(&mut self, num_of_vertices: usize) {
        if self.owners.len() < num_of_vertices {
            self.owners.resize(num_of_vertices, Player::Even);
        }
        if self.priorities.len() < num_of_vertices {
            self.priorities.resize_with(num_of_vertices, || Priority::new(0));
        }
    }

    /// Removes duplicate edges in place, merging the configurations of edges that
    /// become duplicates (same `from`/`to`) via BDD `or`, via
    /// [`merc_collections::dedup_by_bucket`]'s merge mode (see its docs).
    fn remove_duplicates(&mut self) {
        let num_vertices = self.num_of_vertices;
        let num_edges = self.edges_from.len();

        if num_edges == 0 {
            return;
        }

        // `on_entry` gets the edge's original index, so its configuration can be
        // taken straight out of this by index.
        let mut configurations: Vec<Option<BDDFunction>> = std::mem::take(&mut self.edges_configuration)
            .into_iter()
            .map(Some)
            .collect();

        let mut new_edges_from = ByteCompressedVec::with_capacity(num_edges, num_vertices.bytes_required());
        let mut new_edges_to = ByteCompressedVec::with_capacity(num_edges, num_vertices.bytes_required());
        let mut new_configuration: Vec<BDDFunction> = Vec::with_capacity(num_edges);

        dedup_by_bucket(
            num_vertices,
            num_edges,
            |i| self.edges_from.index(i).value(),
            |i| self.edges_to.index(i),
            |i, from, outcome| {
                let configuration = configurations[i]
                    .take()
                    .expect("every edge's configuration is taken exactly once");

                match outcome {
                    DedupOutcome::Keep { .. } => {
                        new_edges_from.push(VertexIndex::new(from));
                        new_edges_to.push(self.edges_to.index(i));
                        new_configuration.push(configuration);
                    }
                    DedupOutcome::Duplicate { position } => {
                        new_configuration[position] = new_configuration[position]
                            .or(&configuration)
                            .expect("Duplicate edges should have compatible BDD managers");
                    }
                }
            },
        );

        new_edges_from.shrink_to_fit();
        new_edges_to.shrink_to_fit();
        new_configuration.shrink_to_fit();

        self.edges_from = new_edges_from;
        self.edges_to = new_edges_to;
        self.edges_configuration = new_configuration;
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use oxidd::BooleanFunction;
    use oxidd::Manager;
    use oxidd::ManagerRef;
    use oxidd::bdd::BDDFunction;
    use rand::RngExt;

    use merc_symbolic::random_bdd;
    use merc_utilities::random_test;

    use crate::PG;
    use crate::ParityGameBuilder;
    use crate::Player;
    use crate::Priority;
    use crate::VariabilityParityGameBuilder;
    use crate::VertexIndex;

    #[test]
    #[cfg_attr(miri, ignore)] // bitvec is incompatible with miri.
    fn test_random_remove_duplicates() {
        random_test(100, |rng| {
            let num_of_vertices = rng.random_range(1..20);
            let mut builder = ParityGameBuilder::new(VertexIndex::new(0));
            for v in 0..num_of_vertices {
                builder.add_vertex(VertexIndex::new(v), Player::Even, Priority::new(0));
            }
            for v in 0..num_of_vertices {
                for _ in 0..rng.random_range(0..10) {
                    let to = rng.random_range(0..num_of_vertices);
                    builder.add_edge(VertexIndex::new(v), VertexIndex::new(to));
                }
            }

            let game = builder.finish(false, true);
            for vertex in game.iter_vertices() {
                let targets: Vec<_> = game.outgoing_edges(vertex).map(|e| e.to()).collect();
                assert!(
                    targets.iter().all_unique(),
                    "Outgoing edges of {vertex} should be unique"
                );
            }
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // oxidd is incompatible with miri.
    fn test_random_variability_remove_duplicates() {
        random_test(20, |rng| {
            let manager_ref = oxidd::bdd::new_manager(2048, 1024, 1);
            let variables: Vec<BDDFunction> = manager_ref.with_manager_exclusive(|manager| {
                manager
                    .add_vars(3)
                    .map(|i| BDDFunction::var(manager, i))
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            });

            let num_of_vertices = rng.random_range(1..15);
            let mut builder = VariabilityParityGameBuilder::with_capacity(VertexIndex::new(0), 0);
            for v in 0..num_of_vertices {
                builder.add_vertex(VertexIndex::new(v), Player::Even, Priority::new(0));
            }

            for v in 0..num_of_vertices {
                for _ in 0..rng.random_range(0..10) {
                    let to = VertexIndex::new(rng.random_range(0..num_of_vertices));
                    let configuration = random_bdd(&manager_ref, rng, &variables, 5).unwrap();
                    builder.add_edge(VertexIndex::new(v), configuration, to);
                }
            }

            let configuration = random_bdd(&manager_ref, rng, &variables, 5).unwrap();
            let game = builder.finish(&manager_ref, configuration, variables, true);

            for vertex in game.iter_vertices() {
                let targets: Vec<_> = game.outgoing_edges(vertex).map(|e| e.to()).collect();
                assert!(
                    targets.iter().all_unique(),
                    "Outgoing edges of {vertex} should be unique after merging"
                );
            }
        });
    }

    /// Deterministic check that duplicate edges are actually *merged* (BDD `or`),
    /// not just deduplicated down to an arbitrary survivor.
    #[test]
    #[cfg_attr(miri, ignore)] // oxidd is incompatible with miri.
    fn test_variability_remove_duplicates_merges_configurations() {
        let manager_ref = oxidd::bdd::new_manager(2048, 1024, 1);
        let (a, b) = manager_ref.with_manager_exclusive(|manager| {
            let vars = manager
                .add_vars(2)
                .map(|i| BDDFunction::var(manager, i))
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            (vars[0].clone(), vars[1].clone())
        });

        let mut builder = VariabilityParityGameBuilder::with_capacity(VertexIndex::new(0), 2);
        builder.add_vertex(VertexIndex::new(0), Player::Even, Priority::new(0));
        builder.add_vertex(VertexIndex::new(1), Player::Even, Priority::new(0));

        // Two edges 0 -> 1 with different configurations should merge into a
        // single edge whose configuration is their disjunction.
        builder.add_edge(VertexIndex::new(0), a.clone(), VertexIndex::new(1));
        builder.add_edge(VertexIndex::new(0), b.clone(), VertexIndex::new(1));

        let expected = a.or(&b).unwrap();
        let game = builder.finish(&manager_ref, expected.clone(), vec![a, b], true);

        let edges: Vec<_> = game.outgoing_edges(VertexIndex::new(0)).collect();
        assert_eq!(edges.len(), 1, "Duplicate edges should merge into a single edge");
        assert_eq!(edges[0].to(), VertexIndex::new(1));
        // `BDDFunction` doesn't implement `Debug`, so a plain equality check.
        assert!(
            *edges[0].label() == expected,
            "Merged configuration should be the disjunction of the originals"
        );
    }
}
