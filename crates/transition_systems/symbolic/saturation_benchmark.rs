//! Peak-versus-final diagram size across [`ExplorationStrategy`] variants.
//!
//! Node-wise saturation's whole point is that the *peak* number of live manager nodes over a
//! reachability run stays close to the *final* reachable-set diagram, unlike the whole-set fixpoint
//! schedules (`BreadthFirst`, `Chaining`, `Fixpoint`, `FixpointChaining`), which keep large
//! intermediate frontier diagrams alive simultaneously with the growing state set. See
//! `docs/saturation-implementation-plan.md`, Phase 3.
//!
//! Peak node counts are *not* tracked or asserted on here: getting a precise peak would mean
//! instrumenting `LDDFunction::saturate` itself, inside the `oxidd` fork, which isn't something worth
//! forking `oxidd` for. Instead, build with the `metrics` feature enabled to have
//! `reachability_with_options` log `manager.num_inner_nodes()` — an approximation of table size, since
//! it counts every live node in the manager, relations and metas included — plus oxidd's own
//! per-`LDDOp` call/cache-query/cache-hit counters, once per outer iteration/round, so the peak is a
//! manual, eyeballed reading rather than an asserted number:
//!
//! ```text
//! cargo test --release -p merc_symbolic --features metrics --test saturation_benchmark -- --ignored --nocapture
//! ```
//!
//! The node-count line is logged via `log::info!`, like the rest of this crate's progress reporting,
//! so — unlike oxidd's own op counters below, which go straight to stderr — it needs a logger
//! configured to be visible; `cargo test` binaries don't configure one, so under the invocation above
//! only the op-counter lines actually appear. Run the same fixture through `merc-lps`/`merc-sym`
//! (which do configure a logger) with `--features metrics` and `RUST_LOG=info` to see both.
//!
//! This deliberately deviates from the plan's Phase 3 "done when", which called for an always-run
//! regression test asserting `Saturation`'s peak strictly below `BreadthFirst`'s: without a fork
//! change there is nothing precise enough to assert on, and adding one for a benchmark's sake was
//! judged not worth carrying upstream. The evidence for the claim is this printed table instead.
//!
//! `final_nodes` in the printed table comes from [`oxidd::Function::node_count`] (pre-existing
//! upstream API, unrelated to the peak logging above): the size of the diagram rooted at the final
//! reachable set, as opposed to [`LDDFunction::len`]'s vector count.

use merc_symbolic::ExplorationStrategy;
use merc_symbolic::LddLenCache;
use merc_symbolic::ReachabilityOptions;
use merc_symbolic::SymbolicLPS;
use merc_symbolic::ldd_len;
use merc_symbolic::read_sylvan;
use merc_utilities::Timing;
use oxidd::Function;

/// One fixture family: `(name, .ldd bytes, manager inner-node capacity)`.
///
/// Capacities are sized generously above the largest final reachable set the fixture will produce,
/// per the Phase 3 note that the default test manager (`new_manager(2048, 1024, 1)`) is too small for
/// these larger benchmark families.
///
/// Deliberately excludes `anderson.8`, `bakery.6`, `bakery.7`, `lifts.6` and `lifts.7`:
/// `BreadthFirst`'s peak on these is large enough to be OOM-killed on a machine with tens of GB free
/// (confirmed for `anderson.8`) rather than just slow, which is fine for the phenomenon this plan
/// exists to demonstrate but not for a table meant to actually finish. Add them back locally, with a
/// generously sized capacity and enough free memory, to see just how bad the blow-up gets.
const FIXTURES: &[(&str, &[u8], usize)] = &[
    (
        "anderson.4",
        include_bytes!("../../../examples/ldd/anderson.4.ldd"),
        1 << 16,
    ),
    (
        "anderson.6",
        include_bytes!("../../../examples/ldd/anderson.6.ldd"),
        1 << 25,
    ),
    (
        "bakery.4",
        include_bytes!("../../../examples/ldd/bakery.4.ldd"),
        1 << 18,
    ),
    (
        "bakery.5",
        include_bytes!("../../../examples/ldd/bakery.5.ldd"),
        1 << 20,
    ),
    (
        "blocks.2",
        include_bytes!("../../../examples/ldd/blocks.2.ldd"),
        1 << 18,
    ),
    (
        "blocks.3",
        include_bytes!("../../../examples/ldd/blocks.3.ldd"),
        1 << 20,
    ),
    (
        "collision.4",
        include_bytes!("../../../examples/ldd/collision.4.ldd"),
        1 << 18,
    ),
    (
        "collision.5",
        include_bytes!("../../../examples/ldd/collision.5.ldd"),
        1 << 20,
    ),
    (
        "schedule_world.2",
        include_bytes!("../../../examples/ldd/schedule_world.2.ldd"),
        1 << 18,
    ),
    (
        "schedule_world.3",
        include_bytes!("../../../examples/ldd/schedule_world.3.ldd"),
        1 << 18,
    ),
];

const STRATEGIES: &[ExplorationStrategy] = &[
    ExplorationStrategy::BreadthFirst,
    ExplorationStrategy::Chaining,
    ExplorationStrategy::Fixpoint,
    ExplorationStrategy::FixpointChaining,
    ExplorationStrategy::Saturation,
];

/// Runs reachability over `bytes` with `strategy`, logging node counts and oxidd's own statistics along
/// the way when built with `--features metrics` (see the module doc comment), and returns `(reachable
/// state count, final diagram node count)`.
fn run(bytes: &[u8], capacity: usize, strategy: ExplorationStrategy) -> (usize, usize) {
    let storage = oxidd::ldd::new_manager(capacity, capacity / 2, 1);
    let mut lts = read_sylvan(&storage, &mut &bytes[..]).expect("loading a checked-in fixture should not fail");

    let options = ReachabilityOptions {
        strategy,
        // The Sylvan fixtures have their relations pregenerated, so there is nothing to cache.
        cached: false,
        ..ReachabilityOptions::default()
    };
    let mut context = lts.create_context();
    let result = merc_symbolic::reachability_with_options(&storage, &mut lts, &mut context, &options, &Timing::new())
        .expect("reachability should succeed on a checked-in fixture");

    let states = ldd_len(&result.states, &mut LddLenCache::new())
        .exact()
        .expect("the fixtures are small") as usize;
    (states, result.states.node_count())
}

/// Sweeps every fixture in [FIXTURES] over every strategy in [STRATEGIES] and prints a final
/// `fixture, strategy, states, final_nodes` table; build with `--features metrics` (see the module doc
/// comment) to also see per-iteration/round node counts and oxidd's own statistics along the way. Slow
/// (multiple large fixtures times five strategies), so `#[ignore]`d.
#[test]
#[ignore]
fn saturation_peak_benchmark() {
    println!(
        "{:<18} {:<18} {:>12} {:>12}",
        "fixture", "strategy", "states", "final_nodes"
    );
    for (name, bytes, capacity) in FIXTURES {
        for &strategy in STRATEGIES {
            let (states, final_nodes) = run(bytes, *capacity, strategy);
            println!("{name:<18} {strategy:<18?} {states:>12} {final_nodes:>12}");
        }
    }
}
