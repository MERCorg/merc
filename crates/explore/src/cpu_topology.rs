use std::collections::VecDeque;
use std::fmt;
use std::fmt::Display;
use std::sync::Barrier;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use core_affinity2::CoreId;
use crossbeam_utils::CachePadded;
use log::debug;
use quanta::Clock;

use merc_utilities::MercError;

/// Tuning knobs for [`CpuTopology::detect_with`].
///
/// The defaults trade a fraction of a second of startup latency measurement for a topology
/// that is stable enough to base steal-order/pinning decisions on.
#[derive(Debug, Clone, Copy)]
pub struct TopologyDetectionConfig {
    /// Untimed round trips exchanged before timing starts, to warm up caches and branch predictors.
    pub warmup_rounds: u64,
    /// Timed round trips per sample; the elapsed time divided by this count gives one-way latency.
    pub timed_rounds: u64,
    /// Number of samples taken per core pair; the minimum is kept since noise only inflates it.
    pub samples: usize,
    /// Multiplier on the observed minimum latency used as the clustering threshold.
    pub cluster_factor: f64,
}

impl Default for TopologyDetectionConfig {
    fn default() -> Self {
        TopologyDetectionConfig {
            warmup_rounds: 1_000,
            timed_rounds: 3_000,
            samples: 5,
            cluster_factor: 1.5,
        }
    }
}

/// CPU topology derived from measured pairwise inter-core communication latency.
///
/// Latency is measured directly via cache-line ping-pong rather than parsed from
/// platform-specific CPUID/sysfs data, so it captures whatever actually affects
/// cross-core communication cost on the running machine (SMT siblings, shared
/// L3/CCX, sockets) without needing to know the underlying topology names.
#[derive(Debug, Clone)]
pub struct CpuTopology {
    cores: Vec<CoreId>,
    /// Row-major `num_cores() * num_cores()` one-way latency matrix; symmetric, zero diagonal.
    latency_ns: Vec<f64>,
    /// Disjoint groups of core indices with mutually low latency, ordered by smallest member.
    clusters: Vec<Vec<usize>>,
}

impl CpuTopology {
    /// Detects the topology of the current machine using the default [`TopologyDetectionConfig`].
    pub fn detect() -> Result<CpuTopology, MercError> {
        CpuTopology::detect_with(&TopologyDetectionConfig::default())
    }

    /// Detects the topology of the current machine, measuring latency with the given `config`.
    ///
    /// Falls back to a trivial single-core topology if fewer than two cores are available for
    /// pinning. Measurement uses [`quanta::Clock`], which transparently falls back from the TSC
    /// to the OS monotonic clock on VMs without a stable time-stamp counter.
    pub fn detect_with(config: &TopologyDetectionConfig) -> Result<CpuTopology, MercError> {
        let cores = core_affinity2::get_core_ids().unwrap_or_default();
        if cores.len() < 2 {
            debug!(
                "cpu topology detection skipped: fewer than 2 cores available ({})",
                cores.len()
            );
            let num_cores = cores.len();
            return CpuTopology::from_latency_matrix(cores, vec![0.0; num_cores * num_cores], config.cluster_factor);
        }

        let latency_ns = measure_all_pairs(&cores, config);
        CpuTopology::from_latency_matrix(cores, latency_ns, config.cluster_factor)
    }

    /// Builds a [`CpuTopology`] from a precomputed latency matrix, validating its shape.
    ///
    /// `latency_ns` must be a row-major `cores.len() * cores.len()` matrix that is symmetric
    /// with a zero diagonal and non-negative entries. Both [`CpuTopology::detect_with`] and
    /// the unit tests funnel through this constructor.
    pub(crate) fn from_latency_matrix(
        cores: Vec<CoreId>,
        latency_ns: Vec<f64>,
        cluster_factor: f64,
    ) -> Result<CpuTopology, MercError> {
        validate_latency_matrix(&cores, &latency_ns)?;

        let clusters = cluster_by_latency(&latency_ns, cores.len(), cluster_factor);
        Ok(CpuTopology {
            cores,
            latency_ns,
            clusters,
        })
    }

    /// Number of cores this topology was measured over.
    pub fn num_cores(&self) -> usize {
        self.cores.len()
    }

    /// The measured cores, in the order used by all core-index accessors.
    pub fn cores(&self) -> &[CoreId] {
        &self.cores
    }

    /// One-way latency, in nanoseconds, between core indices `a` and `b`.
    ///
    /// # Panics
    ///
    /// Panics if `a` or `b` is not less than [`CpuTopology::num_cores`].
    pub fn latency_ns(&self, a: usize, b: usize) -> f64 {
        self.latency_ns[a * self.cores.len() + b]
    }

    /// Disjoint groups of core indices with mutually low latency.
    pub fn clusters(&self) -> &[Vec<usize>] {
        &self.clusters
    }

    /// Other core indices ordered nearest-to-farthest from `core`, index order breaking ties.
    ///
    /// This is the victim order a topology-aware work-stealer would use.
    pub fn cores_by_proximity(&self, core: usize) -> Vec<usize> {
        let mut others: Vec<usize> = (0..self.num_cores()).filter(|&index| index != core).collect();
        others.sort_by(|&a, &b| {
            self.latency_ns(core, a)
                .total_cmp(&self.latency_ns(core, b))
                .then_with(|| a.cmp(&b))
        });
        others
    }
}

impl Display for CpuTopology {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "cpu clusters:")?;
        for (index, cluster) in self.clusters.iter().enumerate() {
            let members = cluster
                .iter()
                .map(|&core| format!("core{}", self.cores[core].0))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(f, "  {index}: [{members}]")?;
        }
        Ok(())
    }
}

/// Validates that `latency_ns` is a well-formed symmetric zero-diagonal matrix over `cores`.
fn validate_latency_matrix(cores: &[CoreId], latency_ns: &[f64]) -> Result<(), MercError> {
    let n = cores.len();
    if latency_ns.len() != n * n {
        return Err(MercError::from(format!(
            "latency matrix has {} entries, expected {} for {n} cores",
            latency_ns.len(),
            n * n
        )));
    }

    for i in 0..n {
        for j in 0..n {
            let value = latency_ns[i * n + j];
            if value < 0.0 {
                return Err(MercError::from(format!("negative latency at ({i}, {j}): {value}")));
            }
            if i == j && value != 0.0 {
                return Err(MercError::from(format!(
                    "non-zero diagonal latency at ({i}, {i}): {value}"
                )));
            }

            let mirrored = latency_ns[j * n + i];
            if value != mirrored {
                return Err(MercError::from(format!(
                    "asymmetric latency matrix: ({i}, {j}) = {value} but ({j}, {i}) = {mirrored}"
                )));
            }
        }
    }

    Ok(())
}

/// Groups core indices into connected components of the "near" graph (single linkage), where an
/// edge connects two cores if their latency is within `factor` times the observed minimum.
///
/// SMT/shared-L3 pairs sit 1.5-3x below cross-CCX/socket latencies in practice, so anchoring the
/// threshold to the observed minimum is scale-independent across machines.
fn cluster_by_latency(latency_ns: &[f64], num_cores: usize, factor: f64) -> Vec<Vec<usize>> {
    if num_cores <= 1 {
        return (0..num_cores).map(|core| vec![core]).collect();
    }

    let threshold = minimum_off_diagonal_latency(latency_ns, num_cores) * factor;

    let mut visited = vec![false; num_cores];
    let mut clusters = Vec::new();
    for start in 0..num_cores {
        if visited[start] {
            continue;
        }

        let mut component = connected_component(start, latency_ns, num_cores, threshold, &mut visited);
        component.sort_unstable();
        clusters.push(component);
    }

    clusters.sort_by_key(|cluster| cluster[0]);
    clusters
}

/// Returns the smallest observed off-diagonal one-way latency.
fn minimum_off_diagonal_latency(latency_ns: &[f64], num_cores: usize) -> f64 {
    let mut min_latency = f64::INFINITY;
    for i in 0..num_cores {
        for j in 0..num_cores {
            if i != j {
                min_latency = min_latency.min(latency_ns[i * num_cores + j]);
            }
        }
    }
    min_latency
}

/// Single-linkage flood fill: returns every core reachable from `start` through
/// edges of latency at most `threshold`, marking the reached cores as visited.
fn connected_component(
    start: usize,
    latency_ns: &[f64],
    num_cores: usize,
    threshold: f64,
    visited: &mut [bool],
) -> Vec<usize> {
    let mut component = Vec::new();
    let mut queue = VecDeque::new();
    queue.push_back(start);
    visited[start] = true;

    while let Some(node) = queue.pop_front() {
        component.push(node);
        for neighbor in 0..num_cores {
            if !visited[neighbor] && latency_ns[node * num_cores + neighbor] <= threshold {
                visited[neighbor] = true;
                queue.push_back(neighbor);
            }
        }
    }

    component
}

/// Measures the row-major one-way latency matrix over `cores`, sequentially pair by pair.
///
/// Pairs are measured one at a time because concurrently running pairs would perturb each
/// other's cache traffic and skew the results.
fn measure_all_pairs(cores: &[CoreId], config: &TopologyDetectionConfig) -> Vec<f64> {
    let n = cores.len();
    let mut latency_ns = vec![0.0; n * n];

    let clock = Clock::new();
    let _ = clock.now(); // Force quanta's lazy TSC calibration outside the timed regions below.

    for i in 0..n {
        for j in (i + 1)..n {
            let one_way_ns = measure_pair(&clock, cores[i], cores[j], config);
            latency_ns[i * n + j] = one_way_ns;
            latency_ns[j * n + i] = one_way_ns;
            debug!(
                "measured latency core {} <-> core {}: {one_way_ns:.1} ns",
                cores[i].0, cores[j].0
            );
        }
    }

    latency_ns
}

/// Measures one-way latency between `core_a` and `core_b` via cache-line ping-pong.
///
/// Spawns two fresh threads pinned to `core_a` and `core_b`, never touching the caller's own
/// affinity since the caller may itself be a pinned rayon worker. Pinning is best-effort: a
/// failure is logged and measurement proceeds unpinned, so that a pin error can never leave one
/// thread stranded at a barrier the other never reaches.
fn measure_pair(clock: &Clock, core_a: CoreId, core_b: CoreId, config: &TopologyDetectionConfig) -> f64 {
    let counter = CachePadded::new(AtomicU64::new(0));
    // Every phase (warmup and each timed sample) is bracketed by barriers so that the counter is
    // only ever reset while both threads are parked, never racing a thread's trailing store.
    let barrier = Barrier::new(2);

    std::thread::scope(|scope| {
        let responder = scope.spawn(|| {
            pin_current_thread(core_b);

            barrier.wait();
            run_rounds(&counter, false, config.warmup_rounds);
            barrier.wait();

            for _ in 0..config.samples {
                barrier.wait();
                run_rounds(&counter, false, config.timed_rounds);
                barrier.wait();
            }
        });

        let initiator = scope.spawn(|| {
            pin_current_thread(core_a);

            barrier.wait();
            run_rounds(&counter, true, config.warmup_rounds);
            barrier.wait();

            let mut min_latency_ns = f64::INFINITY;
            for _ in 0..config.samples {
                counter.store(0, Ordering::Release);
                barrier.wait();

                let start = clock.raw();
                run_rounds(&counter, true, config.timed_rounds);
                let end = clock.raw();

                barrier.wait();

                let elapsed_ns = clock.delta(start, end).as_nanos() as f64;
                let one_way_ns = elapsed_ns / (2.0 * config.timed_rounds as f64);
                min_latency_ns = min_latency_ns.min(one_way_ns);
            }
            min_latency_ns
        });

        responder.join().expect("responder thread panicked");
        initiator.join().expect("initiator thread panicked")
    })
}

/// Pins the current thread to `core`, logging at debug level if the platform refuses.
fn pin_current_thread(core: CoreId) {
    if let Err(error) = core.set_affinity() {
        debug!("failed to pin latency measurement thread to core {}: {error}", core.0);
    }
}

/// Runs `rounds` full ping-pong round trips over the shared `counter`.
///
/// The initiator writes odd values, the responder writes even values, so each `+1` is one
/// cross-core cache-line transfer and each full round trip is two transfers.
fn run_rounds(counter: &AtomicU64, is_initiator: bool, rounds: u64) {
    let mut expected: u64 = if is_initiator { 0 } else { 1 };
    for _ in 0..rounds {
        while counter.load(Ordering::Acquire) != expected {
            std::hint::spin_loop();
        }
        counter.store(expected + 1, Ordering::Release);
        expected += 2;
    }
}

#[cfg(test)]
mod tests {
    use super::CpuTopology;
    use super::TopologyDetectionConfig;
    use super::cluster_by_latency;

    use core_affinity2::CoreId;

    /// Builds a symmetric zero-diagonal `n x n` matrix from an off-diagonal filler function.
    fn matrix(n: usize, mut latency: impl FnMut(usize, usize) -> f64) -> Vec<f64> {
        let mut values = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..n {
                if i != j {
                    values[i * n + j] = latency(i, j);
                }
            }
        }
        values
    }

    #[test]
    fn cluster_by_latency_smt_pairs() {
        // Cores (0, 1) and (2, 3) are SMT siblings at 20 ns; everything else is 100 ns.
        let values = matrix(4, |i, j| {
            if (i, j) == (0, 1) || (i, j) == (1, 0) || (i, j) == (2, 3) || (i, j) == (3, 2) {
                20.0
            } else {
                100.0
            }
        });

        let clusters = cluster_by_latency(&values, 4, 1.5);
        assert_eq!(clusters, vec![vec![0, 1], vec![2, 3]]);
    }

    #[test]
    fn cluster_by_latency_two_socket() {
        // Cores 0..4 share a socket at 25 ns, cores 4..8 share the other socket at 25 ns,
        // cross-socket pairs are 200 ns.
        let values = matrix(8, |i, j| if (i < 4) == (j < 4) { 25.0 } else { 200.0 });

        let clusters = cluster_by_latency(&values, 8, 1.5);
        assert_eq!(clusters, vec![vec![0, 1, 2, 3], vec![4, 5, 6, 7]]);
    }

    #[test]
    fn cluster_by_latency_uniform_is_one_cluster() {
        let values = matrix(4, |_, _| 50.0);

        let clusters = cluster_by_latency(&values, 4, 1.5);
        assert_eq!(clusters, vec![vec![0, 1, 2, 3]]);
    }

    #[test]
    fn cluster_by_latency_chain_links_transitively() {
        // 0-1 and 1-2 are near (10 ns), but 0-2 is far (100 ns). Single linkage still
        // merges all three into one cluster because 1 bridges 0 and 2.
        let values = matrix(3, |i, j| if i == 1 || j == 1 { 10.0 } else { 100.0 });

        let clusters = cluster_by_latency(&values, 3, 1.5);
        assert_eq!(clusters, vec![vec![0, 1, 2]]);
    }

    #[test]
    fn cluster_by_latency_trivial_for_at_most_one_core() {
        assert_eq!(cluster_by_latency(&[], 0, 1.5), Vec::<Vec<usize>>::new());
        assert_eq!(cluster_by_latency(&[0.0], 1, 1.5), vec![vec![0]]);
    }

    #[test]
    fn cores_by_proximity_orders_nearest_first() {
        let values = matrix(4, |i, j| match (i.min(j), i.max(j)) {
            (0, 1) => 10.0,
            (0, 2) => 30.0,
            (0, 3) => 20.0,
            (1, 2) => 15.0,
            (1, 3) => 25.0,
            (2, 3) => 5.0,
            _ => unreachable!(),
        });
        let cores = (0..4).map(CoreId).collect();
        let topology = CpuTopology::from_latency_matrix(cores, values, 1.5).unwrap();

        assert_eq!(topology.cores_by_proximity(0), vec![1, 3, 2]);
    }

    #[test]
    fn cores_by_proximity_breaks_ties_by_index() {
        // Core 0 is equidistant from cores 1 and 2; the lower index must sort first.
        let values = matrix(3, |i, j| if i.min(j) == 0 { 50.0 } else { 5.0 });
        let cores = (0..3).map(CoreId).collect();
        let topology = CpuTopology::from_latency_matrix(cores, values, 1.5).unwrap();

        assert_eq!(topology.cores_by_proximity(0), vec![1, 2]);
    }

    #[test]
    fn from_latency_matrix_rejects_invalid_input() {
        let two = || (0..2).map(CoreId).collect::<Vec<_>>();

        // Wrong size, asymmetric, negative entry, and non-zero diagonal must all be rejected.
        assert!(CpuTopology::from_latency_matrix((0..3).map(CoreId).collect(), vec![0.0; 4], 1.5).is_err());
        assert!(CpuTopology::from_latency_matrix(two(), vec![0.0, 10.0, 20.0, 0.0], 1.5).is_err());
        assert!(CpuTopology::from_latency_matrix(two(), vec![0.0, -1.0, -1.0, 0.0], 1.5).is_err());
        assert!(CpuTopology::from_latency_matrix(two(), vec![1.0, 10.0, 10.0, 0.0], 1.5).is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn detect_with_produces_a_structurally_valid_topology() {
        let config = TopologyDetectionConfig {
            warmup_rounds: 200,
            timed_rounds: 500,
            samples: 2,
            cluster_factor: 1.5,
        };

        let topology = match CpuTopology::detect_with(&config) {
            Ok(topology) => topology,
            Err(error) => panic!("topology detection failed: {error}"),
        };
        let n = topology.num_cores();
        if n < 2 {
            // Not enough cores available in this environment (e.g. a constrained CI container).
            return;
        }

        for i in 0..n {
            assert_eq!(topology.latency_ns(i, i), 0.0);
            for j in 0..n {
                assert_eq!(topology.latency_ns(i, j), topology.latency_ns(j, i));
                if i != j {
                    assert!(topology.latency_ns(i, j) > 0.0);
                }
            }
        }

        let mut covered: Vec<usize> = topology.clusters().iter().flatten().copied().collect();
        covered.sort_unstable();
        assert_eq!(covered, (0..n).collect::<Vec<_>>());
    }
}
