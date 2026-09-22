//! Default sizes and shared command-line argument structs used by the tools that build a decision
//! diagram from CLI input (`merc-sym`, `merc-lps`, `merc-pbes`), so that they can be adapted in a
//! single place.

#[cfg(feature = "clap")]
use merc_tools::KaHyParArgs;
#[cfg(feature = "clap")]
use merc_utilities::MercError;

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

/// The default capacity, in gigabytes, used for [`OxiddArgs::capacity_gib`].
#[cfg(feature = "clap")]
const DEFAULT_OXIDD_CAPACITY_GIB: u32 = 1;

/// Command-line arguments for initialising an Oxidd decision diagram manager, shared by every tool
/// (`merc-sym`, `merc-lps`, `merc-pbes`) that builds a BDD or LDD manager from CLI input.
#[cfg(feature = "clap")]
#[derive(clap::Args, Debug)]
pub struct OxiddArgs {
    /// Number of worker threads for the Oxidd decision diagram manager.
    #[arg(long = "oxidd-workers", global = true, default_value_t = 1)]
    workers: u32,

    /// Capacity of the manager's inner-node table, in gigabytes (as a power of two, i.e. `1 << 30`
    /// bytes per gigabyte).
    #[arg(long = "oxidd-capacity", global = true, default_value_t = DEFAULT_OXIDD_CAPACITY_GIB)]
    capacity_gib: u32,

    /// Capacity of the manager's apply cache, in gigabytes; defaults to `--oxidd-capacity` when omitted.
    #[arg(long = "oxidd-cache-capacity", global = true)]
    cache_capacity_gib: Option<u32>,
}

#[cfg(feature = "clap")]
impl OxiddArgs {
    /// Initializes an Oxidd BDD manager based on these arguments.
    pub fn init_bdd_manager(&self) -> oxidd::bdd::BDDManagerRef {
        oxidd::bdd::new_manager(self.node_capacity(), self.cache_capacity(), self.workers)
    }

    /// Initializes an Oxidd LDD manager based on these arguments.
    pub fn init_ldd_manager(&self) -> oxidd::ldd::LDDManagerRef {
        oxidd::ldd::new_manager(self.node_capacity(), self.cache_capacity(), self.workers)
    }

    /// The configured inner-node capacity, converted from gigabytes to a node count.
    fn node_capacity(&self) -> usize {
        (self.capacity_gib as usize) << 30
    }

    /// The configured apply cache capacity, converted from gigabytes, defaulting to the node capacity.
    fn cache_capacity(&self) -> usize {
        self.cache_capacity_gib
            .map_or_else(|| self.node_capacity(), |gib| (gib as usize) << 30)
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
