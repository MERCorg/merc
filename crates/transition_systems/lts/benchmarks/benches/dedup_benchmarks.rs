//! Times `LtsBuilderMem::finish(.., true)` (bucket-local dedup over byte-compressed
//! columns, see `crates/lts/src/lts_builder.rs`) on synthetic transition streams
//! with a known transition count, duplicate ratio, and out-degree shape.
//!
//! There's no other builder to compare against any more (`LtsBuilderFast` was
//! deleted once every call site migrated to `LtsBuilderMem`), so these track
//! `LtsBuilderMem`'s own performance over time - use `--save-baseline`/`--baseline`
//! across revisions (see the `benchmark` skill) rather than a same-run comparison.
//!
//! Sizes here are kept modest on purpose so the whole suite finishes in a few
//! minutes (see the `benchmark` skill: this isn't part of CI, only run
//! manually) - raise the constants below for a fuller local stress test.

use benchmarks_lts::SyntheticTransitions;
use benchmarks_lts::fill_mem_builder;
use benchmarks_lts::generate_uniform;
use benchmarks_lts::generate_with_hub_states;
use criterion::BatchSize;
use criterion::BenchmarkId;
use criterion::Criterion;
use merc_lts::StateIndex;
use rand::SeedableRng;
use rand::rngs::StdRng;

/// Fixed seed so every run (and every `before`/`after` baseline comparison)
/// dedups the exact same synthetic input.
const SEED: u64 = 0xdedb_5eed_1235_7331;

/// Times `finish(.., true)` on a fresh, identically populated builder, so only
/// the dedup work itself is measured (filling the builder from `input` happens
/// in the untimed `iter_batched` setup closure).
fn bench_mem_builder(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    parameter: usize,
    input: &SyntheticTransitions,
) {
    group.bench_function(BenchmarkId::new("mem", parameter), |b| {
        b.iter_batched(
            || fill_mem_builder(input),
            |mut builder| builder.finish(StateIndex::new(0), true),
            BatchSize::LargeInput,
        );
    });
}

/// Sweeps total transition count (states x fixed out-degree), fixed moderate
/// duplicate ratio and small uniform out-degree.
pub fn bench_dedup_vs_transition_count(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut group = c.benchmark_group("dedup_vs_transition_count");

    const OUT_DEGREE: usize = 8;
    for num_states in [1_000, 10_000, 100_000, 250_000] {
        let input = generate_uniform(&mut rng, num_states, OUT_DEGREE, 0.2);
        bench_mem_builder(&mut group, input.transitions.len(), &input);
    }

    group.finish();
}

/// Sweeps duplicate ratio at a fixed transition count and out-degree.
pub fn bench_dedup_vs_duplicate_ratio(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut group = c.benchmark_group("dedup_vs_duplicate_ratio");

    const NUM_STATES: usize = 50_000;
    const OUT_DEGREE: usize = 8;
    for duplicate_percent in [0, 25, 50, 75, 90] {
        let input = generate_uniform(&mut rng, NUM_STATES, OUT_DEGREE, duplicate_percent as f64 / 100.0);
        bench_mem_builder(&mut group, duplicate_percent, &input);
    }

    group.finish();
}

/// Sweeps state count while holding the *total* transition count roughly
/// constant (out-degree shrinks as state count grows), isolating the
/// per-state bookkeeping cost (`offsets` in `remove_duplicates`) from the cost
/// of the transitions themselves - it should stay linear in state count, not
/// accidentally quadratic.
pub fn bench_dedup_vs_num_states(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut group = c.benchmark_group("dedup_vs_num_states");

    const TOTAL_TRANSITIONS: usize = 200_000;
    for num_states in [100, 1_000, 10_000, 100_000] {
        let out_degree = (TOTAL_TRANSITIONS / num_states).max(1);
        let input = generate_uniform(&mut rng, num_states, out_degree, 0.2);
        bench_mem_builder(&mut group, num_states, &input);
    }

    group.finish();
}

/// A small number of "hub" states receive most of the transitions, exercising
/// `LtsBuilderMem`'s hash-set dedup path (see `HASH_DEDUP_THRESHOLD` in
/// `crates/lts/src/lts_builder.rs`) instead of the linear-scan path used for
/// small buckets, and validates the "out-degree is bounded by the action
/// alphabet, not state count" assumption the bucket-local approach relies on.
pub fn bench_dedup_hub_state(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut group = c.benchmark_group("dedup_hub_state");

    const NUM_STATES: usize = 10_000;
    const BACKGROUND_OUT_DEGREE: usize = 4;
    for hub_out_degree in [100, 1_000, 10_000, 100_000] {
        let input = generate_with_hub_states(&mut rng, NUM_STATES, BACKGROUND_OUT_DEGREE, 1, hub_out_degree, 0.5);
        bench_mem_builder(&mut group, hub_out_degree, &input);
    }

    group.finish();
}
