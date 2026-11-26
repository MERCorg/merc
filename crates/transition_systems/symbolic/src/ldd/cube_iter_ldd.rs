use oxidd::Function;
use oxidd::HasWorkers;
use oxidd::ManagerRef;
use oxidd::WorkerPool;
use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;
use oxidd::ldd::Value;
use streaming_iterator::StreamingIterator;

use crate::parallel_map_reduce;

/// Owned `(value, down, right)` triple of an LDD node.
pub struct Data(pub Value, pub LDDFunction, pub LDDFunction);

// Returns an iterator over all right siblings of the given LDD.
pub fn iter_right(ldd: &LDDFunction) -> IterRight {
    IterRight { current: ldd.clone() }
}

/// Returns an iterator over all vectors contained in the given LDD, including the empty vector when
/// the LDD is the set that contains only that.
pub fn iter(ldd: &LDDFunction) -> Iter {
    Iter {
        vector: Vec::new(),
        current: Vec::new(),
        // Terminals are never pushed: the empty set has no vectors, and the empty vector is
        // reported once through `empty_vector`.
        stack: if ldd.node().is_some() {
            vec![ldd.clone()]
        } else {
            Vec::new()
        },
        empty_vector: ldd.is_empty_vector(),
        finished: false,
    }
}

// Returns an iterator over all nodes in the given LDD. Visits each node only if the predicate holds.
pub fn iter_nodes<P>(ldd: &LDDFunction, filter: P) -> IterNode<P>
where
    P: Fn(&LDDFunction) -> bool,
{
    let mut stack = Vec::new();

    // Skip terminals (empty set / empty vector); they have no node data.
    if ldd.node().is_some() {
        stack.push((ldd.clone(), false));
    }

    IterNode {
        stack,
        predicate: filter,
    }
}

pub struct IterNode<P>
where
    P: Fn(&LDDFunction) -> bool,
{
    stack: Vec<(LDDFunction, bool)>,
    predicate: P,
}

impl<P> Iterator for IterNode<P>
where
    P: Fn(&LDDFunction) -> bool,
{
    type Item = (LDDFunction, Data);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((current, visited)) = self.stack.pop() {
            let (value, down, right) = current.node().expect("only inner nodes are pushed onto the stack");

            if visited {
                return Some((current, Data(value, down, right)));
            }

            // Next time we can actually process the current node.
            self.stack.push((current.clone(), true));

            // Add unvisited children to stack (skip terminals: empty set / empty vector)
            if down.node().is_some() && (self.predicate)(&down) {
                self.stack.push((down, false));
            }

            if right.node().is_some() && (self.predicate)(&right) {
                self.stack.push((right, false));
            }
        }

        None
    }
}

pub struct IterRight {
    current: LDDFunction,
}

impl Iterator for IterRight {
    type Item = Data;

    fn next(&mut self) -> Option<Self::Item> {
        match self.current.node() {
            // Reached the empty set (or empty vector), so iteration stops.
            None => None,
            Some((value, down, right)) => {
                // Progress to the right LDD.
                self.current = right.clone();
                Some(Data(value, down, right))
            }
        }
    }
}

pub struct Iter {
    /// Stores the values of the returned vector.
    vector: Vec<Value>,
    /// Stores the current path in the LDD.
    current: Vec<Value>,
    /// Stores the stack for the depth-first search (only non 'true' or 'false' nodes)
    stack: Vec<LDDFunction>,
    /// Whether the empty vector, the only vector of the LDD, still has to be reported.
    empty_vector: bool,
    /// Indicates whether the iteration is finished.
    finished: bool,
}

impl StreamingIterator for Iter {
    type Item = Vec<Value>;

    fn advance(&mut self) {
        if self.empty_vector {
            self.empty_vector = false;
            self.current.clear();
            return;
        }

        // Find the next vector by going down the chain.
        loop {
            if let Some(current) = self.stack.last() {
                let (value, down, _) = current.node().expect("only inner nodes are pushed onto the stack");
                self.vector.push(value);
                if down.is_empty_vector() {
                    self.current.clear();
                    self.current.extend_from_slice(&self.vector);
                    break; // Stop iteration.
                } else {
                    self.stack.push(down);
                }
            } else {
                // No more elements to iterate.
                self.finished = true;
                return;
            }
        }

        // Go up the chain to find the next right sibling that is not 'false'.
        while let Some(current) = self.stack.pop() {
            self.vector.pop();
            let (_, _, right) = current.node().expect("only inner nodes are pushed onto the stack");

            if !right.is_empty() {
                self.stack.push(right); // This is the first right sibling.
                break;
            }
        }
    }

    fn get(&self) -> Option<&Self::Item> {
        if self.finished { None } else { Some(&self.current) }
    }
}

/// How many tasks per worker [`iter_par`] aims for, so that uneven sub-sets still balance.
const TASKS_PER_WORKER: usize = 8;

/// The number of levels that [`iter_par`] expands at most to find enough tasks.
const MAX_SPLIT_LEVELS: usize = 8;

/// Calls `f` for every vector of `ldd` on the workers of `storage`, see [`iter`].
///
/// The top levels of `ldd` are expanded into sub-sets until there are a few per worker (or
/// [`MAX_SPLIT_LEVELS`] levels are expanded), and the sub-sets are divided over the workers with
/// `join`; each is walked sequentially with [`iter`]. Every sub-set gets its own accumulator from
/// `identity`, which gets the manager, and the accumulators are combined with `merge`, which must be
/// associative; `f` is called with the accumulator of its sub-set. The first error that `f` or
/// `merge` returns is returned, and the other sub-sets still run to completion.
///
/// All calls of `f` happen on the workers, so `f` can keep per-worker state in a
/// [`merc_unsafety::WorkerLocal`].
pub fn iter_par<A, E, I, F, M>(storage: &LDDManagerRef, ldd: &LDDFunction, identity: I, f: F, merge: M) -> Result<A, E>
where
    A: Send,
    E: Send,
    I: for<'id> Fn(&<LDDFunction as Function>::Manager<'id>) -> A + Sync,
    F: Fn(&mut A, &[Value]) -> Result<(), E> + Sync,
    M: Fn(A, A) -> Result<A, E> + Sync,
{
    let workers = storage.workers();
    let target = workers.current_num_threads() * TASKS_PER_WORKER;

    // Every task is a prefix together with the sub-set that continues it.
    let mut tasks: Vec<(Vec<Value>, LDDFunction)> = vec![(Vec::new(), ldd.clone())];
    for _ in 0..MAX_SPLIT_LEVELS {
        if tasks.len() >= target || tasks.iter().all(|(_, set)| set.node().is_none()) {
            break;
        }
        let mut expanded = Vec::with_capacity(tasks.len());
        for (prefix, set) in tasks {
            if set.node().is_none() {
                expanded.push((prefix, set));
                continue;
            }
            for Data(value, down, _) in iter_right(&set) {
                let mut prefix = prefix.clone();
                prefix.push(value);
                expanded.push((prefix, down));
            }
        }
        tasks = expanded;
    }

    storage.with_manager_shared(|manager| {
        let run_task = |(prefix, set): &(Vec<Value>, LDDFunction)| -> Result<A, E> {
            let mut acc = identity(manager);
            let mut vector = prefix.clone();
            let mut suffixes = iter(set);
            while let Some(suffix) = suffixes.next() {
                vector.truncate(prefix.len());
                vector.extend_from_slice(suffix);
                f(&mut acc, &vector)?;
            }
            Ok(acc)
        };

        workers.install(|| parallel_map_reduce(workers, 0..tasks.len(), &|i| run_task(&tasks[i]), &merge))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use oxidd::Manager;

    use merc_utilities::random_test;
    use oxidd::ManagerRef;

    use crate::LDD_CACHE_CAPACITY;
    use crate::LDD_NODE_CAPACITY;
    use crate::from_iter;
    use crate::random_vector_set;

    // Test the iterator implementation.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_random_iter() {
        random_test(100, |rng| {
            let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
            manager.with_manager_exclusive(|m| {
                m.add_vars(10);
            });

            let set = random_vector_set(rng, 32, 10, 10);
            let ldd = from_iter(&manager, set.iter());

            assert!(
                iter(&ldd).count() == set.len(),
                "Number of iterations does not match the number of elements in the set."
            );

            let mut results: Vec<Vec<Value>> = Vec::new();
            let mut it = iter(&ldd);
            while let Some(vector) = it.next() {
                assert!(set.contains(vector), "Found element not in the set.");
                results.push(vector.to_vec());
            }

            results.sort();
            let original_len = results.len();
            results.dedup();
            assert_eq!(original_len, results.len(), "iter returned duplicate vectors.");
        })
    }

    /// `iter_par` visits every vector exactly once, for any number of workers.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_random_iter_par() {
        random_test(30, |rng| {
            for threads in [1, 3] {
                let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, threads);
                manager.with_manager_exclusive(|m| {
                    m.add_vars(10);
                });

                let set = random_vector_set(rng, 64, 6, 4);
                let ldd = from_iter(&manager, set.iter());

                let mut visited: Vec<Vec<Value>> = iter_par(
                    &manager,
                    &ldd,
                    |_| Vec::new(),
                    |acc: &mut Vec<Vec<Value>>, vector| -> Result<(), ()> {
                        assert!(rayon::current_thread_index().is_some(), "f runs on a worker");
                        acc.push(vector.to_vec());
                        Ok(())
                    },
                    |mut a, b| {
                        a.extend(b);
                        Ok(a)
                    },
                )
                .unwrap();
                visited.sort();
                let mut expected: Vec<Vec<Value>> = set.iter().cloned().collect();
                expected.sort();
                expected.dedup();
                assert_eq!(visited, expected);
            }
        })
    }

    /// The set containing only the empty vector has one vector, and the empty set none.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_iter_empty_vector() {
        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 2);
        let ldd = manager.with_manager_shared(|m| LDDFunction::empty_vector(m));
        let mut vectors = iter(&ldd);
        assert_eq!(vectors.next().map(|v| v.len()), Some(0));
        assert!(vectors.next().is_none());

        let empty = manager.with_manager_shared(|m| LDDFunction::empty_set(m));
        assert_eq!(iter(&empty).count(), 0);

        let count = iter_par(
            &manager,
            &ldd,
            |_| 0usize,
            |n, _| -> Result<(), ()> {
                *n += 1;
                Ok(())
            },
            |a, b| Ok(a + b),
        )
        .unwrap();
        assert_eq!(count, 1);
    }
}
