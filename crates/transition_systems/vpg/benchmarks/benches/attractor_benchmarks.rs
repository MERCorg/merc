use criterion::BenchmarkId;
use criterion::Criterion;
use oxidd::Function;
use rand::SeedableRng;
use rand::rngs::StdRng;

use benchmarks_vpg::AttractorCase;
use benchmarks_vpg::generate_attractor_case;
use benchmarks_vpg::silent_attractor_progress;
use merc_symbolic::LDD_CACHE_CAPACITY;
use merc_symbolic::LDD_NODE_CAPACITY;
use merc_symbolic::LddLenCache;
use merc_symbolic::ldd_len;

/// Fixed seed so every run generates and attracts over the exact same synthetic
/// games.
const SEED: u64 = 0xa771_1eaf_5ac7_0125;

/// Times both attractor variants on `case`, reporting the resulting attractor set's cardinality
/// and diagram node count once (untimed, since both variants converge to the same set — see the
/// module doc comment) before the timed comparison.
fn bench_attractor(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    parameter: usize,
    case: &AttractorCase,
) {
    let progress = silent_attractor_progress();
    let v = case.game.vertices();
    let vplayer = case.game.players(v).unwrap();

    let (z, _) = case
        .game
        .attractor(case.alpha, &case.u, v, &vplayer, None, None, &progress)
        .unwrap();
    let mut len_cache = LddLenCache::default();
    eprintln!(
        "  [{parameter}] |V| = {}, |Z| = {}, node_count(Z) = {}",
        ldd_len(v, &mut len_cache),
        ldd_len(&z, &mut len_cache),
        z.node_count()
    );

    group.bench_function(BenchmarkId::new("todo", parameter), |b| {
        b.iter(|| {
            case.game
                .attractor(case.alpha, &case.u, v, &vplayer, None, None, &progress)
                .unwrap()
        });
    });

    group.bench_function(BenchmarkId::new("naive", parameter), |b| {
        b.iter(|| {
            case.game
                .attractor_naive(case.alpha, &case.u, v, &vplayer, None, None, &progress)
                .unwrap()
        });
    });
}

/// Sweeps the number of vertices at fixed out-degree and priority count.
pub fn bench_attractor_vs_num_vertices(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut group = c.benchmark_group("attractor_vs_num_vertices");

    const NUM_PRIORITIES: usize = 6;
    const OUT_DEGREE: usize = 4;
    for num_vertices in [100, 1_000, 5_000, 20_000] {
        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let case = generate_attractor_case(&manager, &mut rng, num_vertices, NUM_PRIORITIES, OUT_DEGREE, 3, 2).unwrap();
        bench_attractor(&mut group, num_vertices, &case);
    }

    group.finish();
}

/// Sweeps out-degree (edge density) at a fixed, moderate vertex and priority count.
pub fn bench_attractor_vs_out_degree(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut group = c.benchmark_group("attractor_vs_out_degree");

    const NUM_VERTICES: usize = 5_000;
    const NUM_PRIORITIES: usize = 6;
    for out_degree in [2, 4, 8, 16] {
        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let case = generate_attractor_case(&manager, &mut rng, NUM_VERTICES, NUM_PRIORITIES, out_degree, 3, 2).unwrap();
        bench_attractor(&mut group, out_degree, &case);
    }

    group.finish();
}

/// Sweeps the number of priorities (and therefore how many `zielonka`-style attractor calls a
/// solve would chain) at a fixed vertex count and out-degree.
pub fn bench_attractor_vs_num_priorities(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut group = c.benchmark_group("attractor_vs_num_priorities");

    const NUM_VERTICES: usize = 5_000;
    const OUT_DEGREE: usize = 4;
    for num_priorities in [2, 4, 8, 16] {
        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let case = generate_attractor_case(&manager, &mut rng, NUM_VERTICES, num_priorities, OUT_DEGREE, 3, 2).unwrap();
        bench_attractor(&mut group, num_priorities, &case);
    }

    group.finish();
}

/// Sweeps the oxidd manager's worker thread count at a fixed game.
///
/// 2-4 threads gave a genuine ~2x speedup over 1, but more threads were slower.
pub fn bench_attractor_vs_threads(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut group = c.benchmark_group("attractor_vs_threads");

    const NUM_VERTICES: usize = 20_000;
    const NUM_PRIORITIES: usize = 6;
    const OUT_DEGREE: usize = 4;
    for threads in [1, 2, 4, 8, 16] {
        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, threads);
        let case = generate_attractor_case(&manager, &mut rng, NUM_VERTICES, NUM_PRIORITIES, OUT_DEGREE, 3, 2).unwrap();
        bench_attractor(&mut group, threads as usize, &case);
    }

    group.finish();
}

/// Sweeps the oxidd manager's inner-node/apply-cache table capacity at a fixed game and thread
/// count.
pub fn bench_attractor_vs_capacity(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut group = c.benchmark_group("attractor_vs_capacity");

    const NUM_VERTICES: usize = 20_000;
    const NUM_PRIORITIES: usize = 6;
    const OUT_DEGREE: usize = 4;
    for capacity_bits in [12, 16, 20, 24] {
        let capacity = 1usize << capacity_bits;
        let manager = oxidd::ldd::new_manager(capacity, capacity, 1);
        let case = generate_attractor_case(&manager, &mut rng, NUM_VERTICES, NUM_PRIORITIES, OUT_DEGREE, 3, 2).unwrap();
        bench_attractor(&mut group, capacity_bits, &case);
    }

    group.finish();
}

/// Sweeps the number of transition relations per owner at a fixed vertex count, out-degree and
/// priority count.
pub fn bench_attractor_vs_num_groups(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut group = c.benchmark_group("attractor_vs_num_groups");

    const NUM_VERTICES: usize = 5_000;
    const NUM_PRIORITIES: usize = 6;
    const OUT_DEGREE: usize = 4;
    for num_groups in [1, 2, 4, 8] {
        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let case = generate_attractor_case(
            &manager,
            &mut rng,
            NUM_VERTICES,
            NUM_PRIORITIES,
            OUT_DEGREE,
            3,
            num_groups,
        )
        .unwrap();
        bench_attractor(&mut group, num_groups, &case);
    }

    group.finish();
}
