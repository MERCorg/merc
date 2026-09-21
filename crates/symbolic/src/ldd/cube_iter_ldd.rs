use oxidd::ldd::LDDFunction;
use oxidd::ldd::Value;
use streaming_iterator::StreamingIterator;

/// Owned `(value, down, right)` triple of an LDD node.
pub struct Data(pub Value, pub LDDFunction, pub LDDFunction);

// Returns an iterator over all right siblings of the given LDD.
pub fn iter_right(ldd: &LDDFunction) -> IterRight {
    IterRight { current: ldd.clone() }
}

// Returns an iterator over all vectors contained in the given LDD.
pub fn iter(ldd: &LDDFunction) -> Iter {
    if ldd.node().is_none() {
        // Either the empty set or the empty vector, both contain no
        // non-trivial vectors to iterate.
        Iter {
            vector: Vec::new(),
            current: Vec::new(),
            stack: Vec::new(),
            finished: false,
        }
    } else {
        Iter {
            vector: Vec::new(),
            current: Vec::new(),
            stack: vec![ldd.clone()],
            finished: false,
        }
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
    /// Indicates whether the iteration is finished.
    finished: bool,
}

impl StreamingIterator for Iter {
    type Item = Vec<Value>;

    fn advance(&mut self) {
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

#[cfg(test)]
mod tests {

    use super::*;

    use merc_utilities::random_test;

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
}
