//! Default sizes for the Oxidd decision diagram managers, so that they can be adapted in a single place.

/// The number of inner nodes that a BDD manager is initialised with.
pub const BDD_NODE_CAPACITY: usize = 1 << 22;

/// The capacity of the apply cache that a BDD manager is initialised with.
pub const BDD_CACHE_CAPACITY: usize = 1 << 22;

/// The number of inner nodes that an LDD manager is initialised with.
pub const LDD_NODE_CAPACITY: usize = 1 << 22;

/// The capacity of the apply cache that an LDD manager is initialised with.
pub const LDD_CACHE_CAPACITY: usize = 1 << 22;
