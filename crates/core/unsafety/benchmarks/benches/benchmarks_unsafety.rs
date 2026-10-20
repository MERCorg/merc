use std::time::Duration;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;

mod sharded_hashmap_benchmarks;

criterion_group!(
    name = benches;
    config = Criterion::default().measurement_time(Duration::new(10, 0)).sample_size(100);
    targets = sharded_hashmap_benchmarks::benchmark_sharded_hashmap,
        sharded_hashmap_benchmarks::benchmark_dashset,
        sharded_hashmap_benchmarks::benchmark_hashers,
);
criterion_main!(benches);
