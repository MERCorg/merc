use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;

mod reachability_benchmarks;

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = reachability_benchmarks::bench_wms,
        reachability_benchmarks::bench_szymanski,
);
criterion_main!(benches);
