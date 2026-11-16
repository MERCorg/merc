//! Default sizes and shared command-line argument structs used by the tools that build a decision
//! diagram from CLI input (`merc-sym`, `merc-lps`, `merc-pbes`), so that they can be adapted in a
//! single place.

#[cfg(feature = "clap")]
use std::ops::ControlFlow;

#[cfg(feature = "clap")]
use oxidd::ldd::LDDFunction;

#[cfg(feature = "clap")]
use crate::KaHyParArgs;
#[cfg(feature = "clap")]
use merc_utilities::MercError;

#[cfg(feature = "clap")]
use crate::ExplorationStrategy;
#[cfg(feature = "clap")]
use crate::Order;
#[cfg(feature = "clap")]
use crate::VariableOrder;
#[cfg(feature = "clap")]
use crate::parse_order;

/// The number of inner nodes that a BDD manager is initialised with.
pub const BDD_NODE_CAPACITY: usize = 1 << 22;

/// The capacity of the apply cache that a BDD manager is initialised with.
pub const BDD_CACHE_CAPACITY: usize = 1 << 22;

/// The number of inner nodes that an LDD manager is initialised with.
pub const LDD_NODE_CAPACITY: usize = 1 << 22;

/// The capacity of the apply cache that an LDD manager is initialised with.
pub const LDD_CACHE_CAPACITY: usize = 1 << 22;

/// Command-line arguments for initialising an Oxidd decision diagram manager, shared by every tool
/// (`merc-sym`, `merc-lps`, `merc-pbes`) that builds a BDD or LDD manager from CLI input.
#[cfg(feature = "clap")]
#[derive(clap::Args, Debug)]
pub struct OxiddArgs {
    /// Number of worker threads, for the decision diagram manager and for explicit exploration.
    #[arg(long = "threads", alias = "oxidd-workers", global = true, default_value_t = 1)]
    threads: u32,

    /// Approximate memory limit of the decision diagram manager in gigabytes, split between the
    /// node table and the apply cache. This is only an estimate based on the entry sizes, not a
    /// hard limit.
    #[arg(long = "memory", short = 'm', global = true, default_value_t = DEFAULT_MEMORY_GIB)]
    memory_gib: f64,

    /// Number of inner nodes in millions, overriding the amount derived from `--memory`.
    #[arg(long = "oxidd-capacity", global = true, value_parser = clap::value_parser!(u64).range(1..))]
    capacity: Option<u64>,

    /// Number of apply cache entries in millions, overriding the amount derived from `--memory`.
    #[arg(long = "oxidd-cache-capacity", global = true, value_parser = clap::value_parser!(u64).range(1..))]
    cache_capacity: Option<u64>,
}

#[cfg(feature = "clap")]
impl OxiddArgs {
    /// The number of worker threads given by `--threads`.
    pub fn threads(&self) -> usize {
        self.threads.max(1) as usize
    }

    /// Initializes an Oxidd BDD manager based on these arguments.
    pub fn init_bdd_manager(&self) -> oxidd::bdd::BDDManagerRef {
        let (nodes, cache) = self.capacities(BDD_TABLE_BYTES);
        oxidd::bdd::new_manager(nodes, cache, self.threads)
    }

    /// Initializes an Oxidd LDD manager based on these arguments.
    pub fn init_ldd_manager(&self) -> oxidd::ldd::LDDManagerRef {
        let (nodes, cache) = self.capacities(LDD_TABLE_BYTES);
        oxidd::ldd::new_manager(nodes, cache, self.threads)
    }
}

/// Command-line arguments selecting how the parameters of the structure being encoded (an LPS's
/// process parameters, or a PBES's equation parameters) are ordered before it is turned into a
/// decision diagram, shared by every tool (`merc-lps`, `merc-pbes`) that exposes `--reorder`.
#[cfg(feature = "clap")]
#[derive(clap::Args, Debug)]
pub struct ReorderArgs {
    /// Reorder the parameters before exploring: 'mince' runs the MINCE algorithm, which requires the
    /// KaHyPar tool, or an explicit order can be given as a whitespace separated string of numbers.
    /// The reachable states are unaffected, only the size of the decision diagrams.
    #[arg(long, default_value_t = Order::None, value_parser = parse_order)]
    reorder: Order,

    #[command(flatten)]
    kahypar: KaHyParArgs,
}

#[cfg(feature = "clap")]
impl ReorderArgs {
    /// Returns the variable order to explore with, resolving the KaHyPar tool when `--reorder` is set.
    pub fn variable_order(&self) -> Result<VariableOrder, MercError> {
        self.reorder.resolve(|| self.kahypar.resolve())
    }
}

/// Command-line argument selecting the strategy used to apply transition groups during LDD-based
/// reachability, shared by every tool (`merc-sym`, `merc-lps`, `merc-pbes`) that performs symbolic
/// reachability via [`crate::reachability_with_options`] or [`crate::reachability_with_callback`].
#[cfg(feature = "clap")]
#[derive(clap::Args, Debug)]
pub struct ExplorationArgs {
    /// Strategy used to apply the transition groups during reachability.
    #[arg(long, short('s'), value_enum, default_value_t = ExplorationStrategy::default())]
    pub strategy: ExplorationStrategy,
}

/// Command-line argument that stops reachability early after a fixed number of iterations, shared
/// by `merc-sym` and `merc-lps`. Not offered by `merc-pbes`: an incomplete parity game cannot be
/// soundly solved, and `--explore-symbolic` keeps the same options as `--solve-symbolic` so the two
/// stay directly comparable (mirroring how the explicit PBES explorer offers no such flag either).
#[cfg(feature = "clap")]
#[derive(clap::Args, Debug)]
pub struct MaxIterationsArgs {
    /// Stop the exploration after this many iterations (rounds for saturation), and report what has
    /// been found until then. The reported states and deadlocks may then be incomplete.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub max_iterations: Option<u32>,
}

#[cfg(feature = "clap")]
impl MaxIterationsArgs {
    /// Returns an `on_iteration` callback for [`crate::reachability_with_callback`] that stops the
    /// exploration once `--max-iterations` rounds have completed, recording the iteration it
    /// stopped at into `stopped_after` so the caller can report the result as incomplete afterwards
    /// (see [`MaxIterationsArgs::warn_if_stopped`]).
    pub fn on_iteration<'a>(
        &self,
        stopped_after: &'a mut Option<usize>,
    ) -> impl FnMut(usize, &LDDFunction) -> ControlFlow<()> + 'a {
        let max_iterations = self.max_iterations;
        move |iteration, _states| match max_iterations {
            Some(max) if iteration >= max as usize => {
                *stopped_after = Some(iteration);
                ControlFlow::Break(())
            }
            _ => ControlFlow::Continue(()),
        }
    }

    /// Logs a warning if `stopped_after` is `Some`, i.e. `--max-iterations` cut the exploration
    /// short, so the reported states and deadlocks may be incomplete.
    pub fn warn_if_stopped(stopped_after: Option<usize>) {
        if let Some(iteration) = stopped_after {
            log::warn!(
                "Stopped after {iteration} iteration(s) because of --max-iterations, the reported \
                 states and deadlocks may be incomplete"
            );
        }
    }
}

// Private helpers that approximate the oxidd table capacities for a memory limit in gigabytes.

/// The default memory limit of a decision diagram manager, in gigabytes.
#[cfg(feature = "clap")]
const DEFAULT_MEMORY_GIB: f64 = 3.0;

#[cfg(feature = "clap")]
impl OxiddArgs {
    /// Approximates the node and apply cache capacities that fit in the memory limit, unless they
    /// are given explicitly.
    fn capacities(&self, bytes: TableBytes) -> (usize, usize) {
        let (node_budget, cache_budget) = split_budget(bytes, gib(self.memory_gib));
        let (nodes, cache) = bytes.capacities(node_budget, cache_budget);
        let nodes = self.capacity.map_or(nodes, |m| (m * 1_000_000) as usize);
        let cache = self.cache_capacity.map_or(cache, |m| (m * 1_000_000) as usize);
        log::info!(
            "oxidd tables: {nodes} inner nodes ({} + ~{} bytes each, {:.2} GiB at most), {cache} apply cache entries ({} bytes each, {:.2} GiB)",
            bytes.inner_node,
            bytes.unique_table,
            (nodes * (bytes.inner_node + bytes.unique_table)) as f64 / (1u64 << 30) as f64,
            bytes.apply_cache_entry,
            (cache * bytes.apply_cache_entry) as f64 / (1u64 << 30) as f64,
        );
        (nodes, cache)
    }
}

/// Converts gigabytes (`1 << 30` bytes) into bytes.
#[cfg(feature = "clap")]
fn gib(g: f64) -> usize {
    (g.max(0.0) * (1u64 << 30) as f64) as usize
}
/// Approximate bytes per entry of the oxidd tables, measured with `size_of` on the current oxidd
/// version.
#[cfg(feature = "clap")]
#[derive(Clone, Copy, Debug)]
struct TableBytes {
    /// Bytes per inner node.
    inner_node: usize,
    /// Approximate bytes per node in the unique table.
    unique_table: usize,
    /// Bytes per apply cache entry.
    apply_cache_entry: usize,
}

#[cfg(feature = "clap")]
const BDD_TABLE_BYTES: TableBytes = TableBytes {
    inner_node: 16,
    unique_table: 9,
    apply_cache_entry: 20,
};

#[cfg(feature = "clap")]
const LDD_TABLE_BYTES: TableBytes = TableBytes {
    inner_node: 16,
    unique_table: 9,
    apply_cache_entry: 28,
};

#[cfg(feature = "clap")]
impl TableBytes {
    /// Returns the number of nodes and cache entries that fit in the given byte budgets. The cache
    /// capacity is rounded down to a power of two, since oxidd rounds it up.
    fn capacities(&self, node_bytes: usize, cache_bytes: usize) -> (usize, usize) {
        let nodes = node_bytes / (self.inner_node + self.unique_table);
        let entries = cache_bytes / self.apply_cache_entry;
        let cache = if entries == 0 { 1 } else { 1usize << entries.ilog2() };
        (nodes, cache)
    }
}

/// Splits the memory between nodes and apply cache as Sylvan does: the cache gets the largest
/// power of two `c` such that `c` nodes and `c` entries fit, the nodes get the rest.
#[cfg(feature = "clap")]
fn split_budget(bytes: TableBytes, memory: usize) -> (usize, usize) {
    let per_pair = bytes.inner_node + bytes.unique_table + bytes.apply_cache_entry;
    let pairs = memory / per_pair;
    let cache_entries = if pairs == 0 { 1 } else { 1usize << pairs.ilog2() };
    let cache_bytes = cache_entries * bytes.apply_cache_entry;
    (memory.saturating_sub(cache_bytes), cache_bytes)
}
