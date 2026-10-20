//! Times [`merc_symbolic::reachability_with_options`] on checked-in `.sym` examples, across every
//! [`ExplorationStrategy`].
//!
//! A fresh manager and a freshly re-read `SymbolicLts` are built per criterion sample (in the
//! untimed `iter_batched` setup), since `learn_successors` mutates the LTS's learned relations in
//! place — reusing one across samples would make every sample after the first a no-op.

use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

use criterion::BatchSize;
use criterion::BenchmarkId;
use criterion::Criterion;

use merc_symbolic::ExplorationStrategy;
use merc_symbolic::LDD_CACHE_CAPACITY;
use merc_symbolic::LDD_NODE_CAPACITY;
use merc_symbolic::ReachabilityOptions;
use merc_symbolic::SymbolicLPS;
use merc_symbolic::reachability_with_options;
use merc_symbolic::read_symbolic_lts;
use merc_utilities::Timing;

/// Path to a checked-in `.sym` example, relative to the workspace root.
fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../examples/lts")
        .join(name)
}

fn bench_example(c: &mut Criterion, group_name: &str, path: PathBuf) {
    let mut group = c.benchmark_group(group_name);

    for strategy in [
        ExplorationStrategy::BreadthFirst,
        ExplorationStrategy::Chaining,
        ExplorationStrategy::Saturation,
        ExplorationStrategy::Fixpoint,
        ExplorationStrategy::FixpointChaining,
    ] {
        let options = ReachabilityOptions {
            strategy,
            ..ReachabilityOptions::default()
        };

        group.bench_function(BenchmarkId::new("strategy", format!("{strategy:?}")), |b| {
            b.iter_batched(
                || {
                    let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
                    let lts = read_symbolic_lts(&manager, File::open(&path).unwrap()).unwrap();
                    (manager, lts)
                },
                |(manager, mut lts)| {
                    let mut context = lts.create_context();
                    reachability_with_options(&manager, &mut lts, &mut context, &options, &Timing::new()).unwrap()
                },
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

pub fn bench_wms(c: &mut Criterion) {
    bench_example(c, "reachability_wms", example("WMS.sym"));
}

pub fn bench_szymanski(c: &mut Criterion) {
    bench_example(c, "reachability_szymanski", example("Szymanski_3-bit_lin_wait_alt.sym"));
}
