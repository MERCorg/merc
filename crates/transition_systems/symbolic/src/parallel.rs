//! Parallel map and map-reduce over index ranges, as balanced trees of `join`s on a
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
    debug_assert!(!range.is_empty(), "the range must not be empty");
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

/// Returns `map(i)` for every `i` in `0..len`, in order, computed as a balanced tree of `join`s on
/// `workers`.
pub fn parallel_map<W, T, E, M>(workers: &W, len: usize, map: &M) -> Result<Vec<T>, E>
where
    W: WorkerPool,
    T: Send,
    E: Send,
    M: Fn(usize) -> Result<T, E> + Sync,
{
    /// Fills `slots`, which hold the results for the indices from `offset` on.
    fn fill<W, T, E, M>(workers: &W, offset: usize, slots: &mut [Option<T>], map: &M) -> Result<(), E>
    where
        W: WorkerPool,
        T: Send,
        E: Send,
        M: Fn(usize) -> Result<T, E> + Sync,
    {
        match slots.len() {
            0 => Ok(()),
            1 => {
                slots[0] = Some(map(offset)?);
                Ok(())
            }
            len => {
                let (first, second) = slots.split_at_mut(len / 2);
                let (a, b) = workers.join(
                    || fill(workers, offset, first, map),
                    || fill(workers, offset + len / 2, second, map),
                );
                a.and(b)
            }
        }
    }

    let mut slots: Vec<Option<T>> = (0..len).map(|_| None).collect();
    fill(workers, 0, &mut slots, map)?;
    Ok(slots
        .into_iter()
        .map(|slot| slot.expect("every slot is filled"))
        .collect())
}
