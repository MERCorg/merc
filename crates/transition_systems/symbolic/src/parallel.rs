//! Parallel map-reduce over index ranges, as a balanced tree of `join`s on a
//! [`WorkerPool`].

use std::ops::Range;

use oxidd::WorkerPool;

/// Computes `map(i)` for every `i` in `range` and combines the results with `reduce`, as a
/// balanced tree of `join`s on `workers`. The range must not be empty.
pub fn parallel_map_reduce<W, T, E, M, R>(workers: &W, range: Range<usize>, map: &M, reduce: &R) -> Result<T, E>
where
    W: WorkerPool,
    T: Send,
    E: Send,
    M: Fn(usize) -> Result<T, E> + Sync,
    R: Fn(T, T) -> Result<T, E> + Sync,
{
    assert!(!range.is_empty(), "the range must not be empty");
    if range.len() == 1 {
        return map(range.start);
    }
    let middle = range.start + range.len() / 2;
    let (a, b) = workers.join(
        || parallel_map_reduce(workers, range.start..middle, map, reduce),
        || parallel_map_reduce(workers, middle..range.end, map, reduce),
    );
    reduce(a?, b?)
}
