use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::ErrorKind;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use duct::cmd;
use itertools::Itertools;
use log::debug;
use log::trace;

use merc_utilities::MercError;

use crate::DependencyGraph;

/// Bipartitions a [Hypergraph] into two blocks.
trait Partitioner {
    /// Returns the block id (`0` or `1`) of every real vertex of `hypergraph`.
    fn partition(&self, hypergraph: &Hypergraph) -> Result<Vec<usize>, MercError>;
}

/// Computes a variable order for the given dependency graph using the MINCE
/// algorithm and the KaHyPar partitioning tool.
pub fn reorder(kahypar_path: &Path, kahypar_ini_path: &Path, graph: &DependencyGraph) -> Result<Vec<usize>, MercError> {
    reorder_with(&KaHyParPartitioner::new(kahypar_path, kahypar_ini_path), graph)
}

/// Computes a variable order using the MINCE algorithm with the given [Partitioner].
fn reorder_with<P: Partitioner>(partitioner: &P, graph: &DependencyGraph) -> Result<Vec<usize>, MercError> {
    debug!("Total span: {}", graph.total_span());

    let vertices = (0..graph.num_of_vertices()).collect::<Vec<usize>>();
    let result = mince(partitioner, &vertices, &[], graph)?;
    debug!("Reordered total span: {}", graph.reorder(&result).total_span());
    Ok(result)
}

/// The recursive MINCE algorithm to compute a partitioning of the given dependency graph.
///
/// # Details
///
/// The `vertices` are the indices of the subgraph that we are considering. The returned
/// order is always a permutation of `vertices`; whenever the subgraph cannot be split
/// (too few vertices, no connecting hyperedge, or a degenerate one-sided partition) the
/// vertices are returned in their current order.
fn mince<P: Partitioner>(
    partitioner: &P,
    vertices: &[usize],
    left_context: &[usize],
    graph: &DependencyGraph,
) -> Result<Vec<usize>, MercError> {
    trace!("MINCE called with vertices: {:?}", vertices);

    // Base case: at most two vertices are already "ordered".
    if vertices.len() <= 2 {
        trace!("MINCE reached base case with vertices: {:?}", vertices);
        return Ok(vertices.to_vec());
    }

    let hypergraph = create_hypergraph(vertices, left_context, graph)?;

    // Without a connecting hyperedge there is nothing to optimize, so keep the order.
    if hypergraph.edges.len() <= 1 {
        trace!("MINCE reached base case with vertices: {:?}", vertices);
        return Ok(vertices.to_vec());
    }

    // `partition[i]` is the block id (0 or 1) of the i-th vertex of `vertices`; the
    // hypergraph keeps the vertex indices aligned with `vertices`.
    let partition = partitioner.partition(&hypergraph)?;
    debug_assert_eq!(
        partition.len(),
        vertices.len(),
        "Partitioner must assign a block to every vertex"
    );
    debug_assert!(partition.iter().all(|&b| b <= 1), "MINCE only supports bipartitioning");

    let left_vertices: Vec<usize> = vertices
        .iter()
        .zip(&partition)
        .filter_map(|(&v, &block)| if block == 0 { Some(v) } else { None })
        .collect();

    let right_vertices: Vec<usize> = vertices
        .iter()
        .zip(&partition)
        .filter_map(|(&v, &block)| if block == 1 { Some(v) } else { None })
        .collect();

    if left_vertices.is_empty() || right_vertices.is_empty() {
        // Degenerate partition with everything on one side; cannot split further.
        trace!("MINCE reached base case with vertices: {:?}", vertices);
        return Ok(vertices.to_vec());
    }

    let mut left = mince(partitioner, &left_vertices, left_context, graph)?;

    let mut new_left_context = left_context.to_vec();
    new_left_context.extend(&left_vertices);
    let mut right = mince(partitioner, &right_vertices, &new_left_context, graph)?;
    left.append(&mut right);

    // Check that the result is a valid permutation of the input vertices.
    if cfg!(debug_assertions) {
        let mut result = left.clone();
        result.sort_unstable();
        let mut expected = vertices.to_vec();
        expected.sort_unstable();

        debug_assert_eq!(result, expected, "Resulting order is not a valid permutation");
    }

    Ok(left)
}

/// A hypergraph representation with indices, edges, and vertex weights.
pub struct Hypergraph {
    /// The number of real vertices, excluding the two pseudo-vertices.
    pub num_vertices: usize,
    /// Indices into the edges vector marking the start of each hyperedge.
    pub indices: Vec<usize>,
    /// The hyperedges, stored as a flat list of vertex indices.
    pub edges: Vec<usize>,
    /// Weights for each vertex in the hypergraph (real vertices followed by the two pseudo-vertices).
    pub weights: Vec<usize>,
}

/// Constructs a hypergraph from the given dependency graph.
///
/// # Details
///
/// The `vertices` are the indices of the subgraph that we are considering.
fn create_hypergraph(
    vertices: &[usize],
    left_context: &[usize],
    graph: &DependencyGraph,
) -> Result<Hypergraph, MercError> {
    let mut hyperedge_indices = Vec::with_capacity(graph.num_of_relations() + 1);
    let mut hyperedges = Vec::new();
    let mut weights = vec![1; vertices.len() + 2]; // +2 for the pseudo-vertices
    for (index, vertex) in vertices.iter().enumerate() {
        // Calculate the total number of edges that the vertex is involved in, and use it as the weight.
        weights[index] = graph
            .relations()
            .filter(|relation| {
                relation.read_vars().any(|j| j == *vertex) || relation.write_vars().any(|j| j == *vertex)
            })
            .count();
    }

    let mut offset = 0usize;

    // Add two pseudo-vertices to represent the "left" and "right" context of the partition.
    let left_pseudo_vertex = vertices.len();
    let right_pseudo_vertex = vertices.len() + 1;

    // They should not contribute to the cut cost.
    weights[left_pseudo_vertex] = 1;
    weights[right_pseudo_vertex] = 1;

    // Make a hyperedge for every relation
    // Track unique edges as sorted lists of local vertex indices
    let mut seen_edges: Vec<Vec<usize>> = Vec::new();

    for relation in graph.relations() {
        // Collect only variables that are in `vertices`, and use their local indices
        let edge_vars: Vec<usize> = relation
            .read_vars()
            .chain(relation.write_vars())
            .map(|j| {
                match vertices.iter().position(|i| *i == j) {
                    Some(local_index) => local_index,
                    None => {
                        // Variable is not in the current subgraph
                        // Check if it is in the left or right context
                        if left_context.contains(&j) {
                            left_pseudo_vertex
                        } else {
                            right_pseudo_vertex
                        }
                    }
                }
            })
            .collect();

        add_edge(
            &mut hyperedge_indices,
            &mut hyperedges,
            &mut offset,
            &mut seen_edges,
            edge_vars,
        );
    }

    hyperedge_indices.push(offset);
    Ok(Hypergraph {
        num_vertices: vertices.len(),
        indices: hyperedge_indices,
        edges: hyperedges,
        weights,
    })
}

/// Adds an edge to the hypergraph, while ensuring that it is not a self-loop, empty, or duplicated.
fn add_edge(
    hyperedge_indices: &mut Vec<usize>,
    hyperedges: &mut Vec<usize>,
    offset: &mut usize,
    seen_edges: &mut Vec<Vec<usize>>,
    mut edge_vars: Vec<usize>,
) {
    // Deduplicate within-edge vertices and normalize order
    edge_vars.sort_unstable();
    edge_vars.dedup();

    if edge_vars.len() <= 1 {
        // Ignore self-loops and empty edges
        return;
    }

    if seen_edges.iter().any(|e| e == &edge_vars) {
        // Ignore duplicated edges
        return;
    }
    seen_edges.push(edge_vars.clone());
    hyperedge_indices.push(*offset);

    // Add the edge to the hypergraph
    for j in edge_vars {
        hyperedges.push(j);
        *offset += 1;
    }
}

/// Bipartitions hypergraphs by invoking the external KaHyPar tool.
pub struct KaHyParPartitioner {
    kahypar_path: PathBuf,
    kahypar_ini_path: PathBuf,
}

impl KaHyParPartitioner {
    /// Creates a partitioner that runs the `kahypar` binary at `kahypar_path` with
    /// the configuration file `kahypar_ini_path`.
    pub fn new(kahypar_path: &Path, kahypar_ini_path: &Path) -> Self {
        KaHyParPartitioner {
            kahypar_path: kahypar_path.to_path_buf(),
            kahypar_ini_path: kahypar_ini_path.to_path_buf(),
        }
    }
}

impl Partitioner for KaHyParPartitioner {
    fn partition(&self, hypergraph: &Hypergraph) -> Result<Vec<usize>, MercError> {
        run_kahypar(&self.kahypar_path, &self.kahypar_ini_path, hypergraph)?;

        let mut partition = read_partition_file()?;
        debug_assert!(partition.iter().all(|x| *x <= 1), "MINCE only supports bipartitioning");

        // KaHyPar also assigns a block to the two pseudo-vertices, which trail the real
        // vertices in the output; drop them so the result lines up with the real vertices.
        if partition.len() < hypergraph.num_vertices {
            return Err(format!(
                "KaHyPar returned {} blocks for {} vertices",
                partition.len(),
                hypergraph.num_vertices
            )
            .into());
        }
        partition.truncate(hypergraph.num_vertices);
        Ok(partition)
    }
}

/// Writes `reorder.hgr`, runs KaHyPar, and removes the temporary file again.
fn run_kahypar(kahypar_path: &Path, kahypar_ini_path: &Path, hypergraph: &Hypergraph) -> Result<(), MercError> {
    const HYPERGRAPH_FILE: &str = "reorder.hgr";

    let result = (|| {
        // Create a file to write the hypergraph to disk in hMetis format.
        let mut file =
            File::create_new(HYPERGRAPH_FILE).map_err(|e| format!("Failed to create file '{HYPERGRAPH_FILE}': {e}"))?;

        // Expected <num_hyperedges> <num_hypernodes> <type> (line 1)
        // type 10 is vertex weights only.
        writeln!(
            &mut file,
            "{} {} 10",
            hypergraph.indices.len() - 1,
            hypergraph.weights.len()
        )?;

        for (from, to) in hypergraph.indices.iter().tuple_windows() {
            let edge = &hypergraph.edges[*from..*to];
            writeln!(&mut file, "{}", edge.iter().map(|i| i + 1).format(" "))?;
        }

        for weight in &hypergraph.weights {
            writeln!(&mut file, "{} ", weight)?;
        }

        file.flush()?;
        cmd!(
            kahypar_path,
            "-h",
            HYPERGRAPH_FILE,
            "-k",
            "2",
            "--objective",
            "cut",
            "--mode",
            "direct",
            "--epsilon",
            "0.01",
            "-w",
            "1",
            "-p",
            kahypar_ini_path
        )
        .run()?;

        Ok::<(), MercError>(())
    })();

    remove_file_if_exists(HYPERGRAPH_FILE)?;
    result
}

/// Reads KaHyPar's partition output and removes the temporary file again.
fn read_partition_file() -> Result<Vec<usize>, MercError> {
    const PARTITION_FILE: &str = "reorder.hgr.part2.epsilon0.01.seed-1.KaHyPar";

    let result = (|| {
        let partition_file = File::open(PARTITION_FILE)?;
        let mut partition = Vec::new();

        for line in BufReader::new(partition_file).lines() {
            let line = line?;
            let block_id: usize = line.trim().parse()?;
            partition.push(block_id);
        }

        Ok::<Vec<usize>, MercError>(partition)
    })();

    remove_file_if_exists(PARTITION_FILE)?;
    result
}

/// Removes the specified file if it exists, ignoring "file not found" errors.
fn remove_file_if_exists(path: &str) -> Result<(), MercError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use rand::RngExt;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use merc_utilities::MercError;
    use merc_utilities::random_test;

    use super::Hypergraph;
    use super::Partitioner;
    use super::reorder_with;
    use crate::DependencyGraph;
    use crate::Relation;

    /// A [Partitioner] for tests that ignores the hypergraph structure and assigns each
    /// vertex to a random block. This exercises the MINCE recursion (including its
    /// degenerate-partition branches) without depending on the external KaHyPar tool.
    struct RandomPartitioner {
        rng: RefCell<StdRng>,
    }

    impl RandomPartitioner {
        fn new(seed: u64) -> Self {
            RandomPartitioner {
                rng: RefCell::new(StdRng::seed_from_u64(seed)),
            }
        }
    }

    impl Partitioner for RandomPartitioner {
        fn partition(&self, hypergraph: &Hypergraph) -> Result<Vec<usize>, MercError> {
            let mut rng = self.rng.borrow_mut();
            Ok((0..hypergraph.num_vertices)
                .map(|_| rng.random_range(0..2usize))
                .collect())
        }
    }

    /// Builds a dependency graph with `num_vertices` vertices and the given read/write relations.
    fn graph_from(num_vertices: usize, relations: &[(Vec<usize>, Vec<usize>)]) -> DependencyGraph {
        let mut rels: Vec<Relation> = relations
            .iter()
            .map(|(read, write)| Relation::new(read.clone(), write.clone()))
            .collect();
        // Make sure the highest index is present so num_of_vertices matches num_vertices.
        if num_vertices > 0 {
            rels.push(Relation::new(vec![num_vertices - 1], vec![]));
        }
        DependencyGraph::new(rels)
    }

    /// Asserts that `order` is a permutation of `0..num_vertices`.
    fn assert_permutation(order: &[usize], num_vertices: usize) {
        let mut sorted = order.to_vec();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            (0..num_vertices).collect::<Vec<usize>>(),
            "MINCE must return a permutation of all vertices"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_mince_random_partition_is_permutation() {
        random_test(100, |rng| {
            // A handful of overlapping relations so the hypergraph has connecting edges.
            let num_vertices = 8;
            let graph = graph_from(
                num_vertices,
                &[
                    (vec![0, 1], vec![2]),
                    (vec![2, 3], vec![4]),
                    (vec![4, 5], vec![6]),
                    (vec![1, 6], vec![7]),
                    (vec![0, 7], vec![3, 5]),
                ],
            );

            let partitioner = RandomPartitioner::new(rng.random());
            let order = reorder_with(&partitioner, &graph).unwrap();
            assert_permutation(&order, num_vertices);
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_mince_disconnected_subgraph_is_permutation() {
        // Two clusters {0,1,2} and {3,4,5} with no hyperedge between them. A random
        // bipartition can therefore split a cluster into a side that, recursively, has
        // more than two vertices but no connecting hyperedge — the C1 regression case.
        random_test(100, |rng| {
            let num_vertices = 6;
            let graph = graph_from(
                num_vertices,
                &[
                    (vec![0, 1], vec![2]),
                    (vec![1, 2], vec![0]),
                    (vec![3, 4], vec![5]),
                    (vec![4, 5], vec![3]),
                ],
            );

            let partitioner = RandomPartitioner::new(rng.random());
            let order = reorder_with(&partitioner, &graph).unwrap();
            assert_permutation(&order, num_vertices);
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_create_hypergraph_dedups_and_skips_self_loops() {
        // Duplicate relations and a single-variable relation (self-loop) must not create edges.
        let graph = graph_from(
            4,
            &[
                (vec![0, 1], vec![2]),
                (vec![0, 1], vec![2]), // duplicate of the previous edge
                (vec![3], vec![3]),    // collapses to a single vertex -> self-loop
            ],
        );

        // Round-trip through reorder_with to make sure the disconnected vertex 3 is kept.
        let partitioner = RandomPartitioner::new(0);
        let order = reorder_with(&partitioner, &graph).unwrap();
        assert_permutation(&order, 4);
    }
}
