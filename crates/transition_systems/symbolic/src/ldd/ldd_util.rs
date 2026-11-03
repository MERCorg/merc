use std::cmp::Ordering;

use merc_utilities::MercError;
use oxidd::Function;
use oxidd::InnerNode;
use oxidd::LevelNo;
use oxidd::Manager;
use oxidd::ManagerRef;
use oxidd::Node;
use oxidd::NodeID;
use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;
use oxidd::ldd::Value;
use oxidd::util::AllocResult;
use oxidd_core::LevelView;
use oxidd_rules_ldd::LDDTerminal;
use rustc_hash::FxHashMap;

use crate::height;

/// Computes the interleaved cartesian product `{ a₀b₀a₁b₁… | a ∈ set_a, b ∈ set_b }`.
///
/// # Details
///
/// Used to build a strategy over the doubled, interleaved state vector `[from₀,
/// to₀, from₁, to₁, …]` out of two vector sets over the plain (non-interleaved)
/// vector. The recursion is:
///
/// > `merge(⊤, b) = b`
/// > `merge(a, ⊤) = a`
/// > `merge(∅, _) = merge(_, ∅) = ∅`
/// > `merge(a, b) = node(a.value, merge(b, a.down), merge(a.right, b))`, otherwise
///
/// where `⊤` is the singleton set containing only the empty vector and `∅` is
/// the empty set. Note the down-branch swaps `a` and `b`: each level of the
/// result takes its value from whichever operand is "in front", which is what
/// produces the interleaving.
///
/// [`LDDFunction::make_node`] does not apply the LDD reduction rule (unlike
/// Sylvan's `lddmc_makenode`), so this function must apply it itself: a node
/// whose `down` branch became empty is not a valid node and collapses to its
/// `right` sibling.
pub fn merge(manager: &LDDManagerRef, a: &LDDFunction, b: &LDDFunction) -> AllocResult<LDDFunction> {
    manager.with_manager_shared(|m| {
        let mut cache = FxHashMap::default();
        let mut relevel_cache = FxHashMap::default();
        merge_rec(m, 0, a, b, &mut cache, &mut relevel_cache)
    })
}

/// Recursive implementation of [merge], producing an LDD that resides at `level`.
fn merge_rec(
    manager: &LddManager<'_>,
    level: LevelNo,
    a: &LDDFunction,
    b: &LDDFunction,
    cache: &mut FxHashMap<(NodeID, NodeID), LDDFunction>,
    relevel_cache: &mut FxHashMap<(NodeID, LevelNo), LDDFunction>,
) -> AllocResult<LDDFunction> {
    let a_edge = a.as_edge(manager);
    let b_edge = b.as_edge(manager);
    let a_node = manager.get_node(a_edge);
    let b_node = manager.get_node(b_edge);
    if a_node.is_terminal(&LDDTerminal::True) {
        return relevel(manager, b, level, relevel_cache);
    }
    if b_node.is_terminal(&LDDTerminal::True) {
        return relevel(manager, a, level, relevel_cache);
    }
    if a_node.is_terminal(&LDDTerminal::Empty) || b_node.is_terminal(&LDDTerminal::Empty) {
        return LDDFunction::empty_set(manager);
    }

    let key = (a_edge.node_id(), b_edge.node_id());
    if let Some(cached) = cache.get(&key) {
        return Ok(cached.clone());
    }

    let (value, down, right) = node_of(manager, a).expect("checked above: neither the empty set nor the empty vector");

    let down_result = merge_rec(manager, level + 1, b, &down, cache, relevel_cache)?;
    let right_result = merge_rec(manager, level, &right, b, cache, relevel_cache)?;

    let down_is_empty = manager
        .get_node(down_result.as_edge(manager))
        .is_terminal(&LDDTerminal::Empty);
    let result = if down_is_empty {
        // A node with an empty `down` branch is not canonical; it collapses to `right`.
        right_result
    } else {
        LDDFunction::make_node(manager, level, value, &down_result, &right_result)?
    };

    cache.insert(key, result.clone());
    Ok(result)
}

/// Rebuilds `set` such that it resides at `level`, or returns it as is when it already does.
///
/// Nodes do not store their level, but they are stored per level, so a sub-LDD
/// that is reused at a different depth must be rebuilt to remain canonical.
fn relevel(
    manager: &LddManager<'_>,
    set: &LDDFunction,
    level: LevelNo,
    cache: &mut FxHashMap<(NodeID, LevelNo), LDDFunction>,
) -> AllocResult<LDDFunction> {
    let Node::Inner(node) = manager.get_node(set.as_edge(manager)) else {
        return Ok(set.clone());
    };

    // A node in the unique table of `level` has its `down` at `level + 1` and its `right` at `level`, so if that table
    // holds a node with the same children and value then the whole LDD already resides at `level`. The level lock
    // must be released before rebuilding, since `make_node` acquires it again.
    let existing = manager
        .level(level)
        .get(&[node.child(0), node.child(1)], node.value())
        .map(|edge| manager.clone_edge(edge));
    if let Some(edge) = existing {
        return Ok(LDDFunction::from_edge(manager, edge));
    }

    relevel_rec(manager, set, level, cache)
}

/// Recursive implementation of [relevel] that rebuilds every node of `set` at `level`, sharing `cache` between calls.
fn relevel_rec(
    manager: &LddManager<'_>,
    set: &LDDFunction,
    level: LevelNo,
    cache: &mut FxHashMap<(NodeID, LevelNo), LDDFunction>,
) -> AllocResult<LDDFunction> {
    let Some((value, down, right)) = node_of(manager, set) else {
        return Ok(set.clone());
    };

    let key = (set.as_edge(manager).node_id(), level);
    if let Some(cached) = cache.get(&key) {
        return Ok(cached.clone());
    }

    let down = relevel_rec(manager, &down, level + 1, cache)?;
    let right = relevel_rec(manager, &right, level, cache)?;
    let result = LDDFunction::make_node(manager, level, value, &down, &right)?;

    cache.insert(key, result.clone());
    Ok(result)
}

/// The manager of [LDDFunction], as borrowed by [ManagerRef::with_manager_shared].
type LddManager<'id> = <LDDFunction as Function>::Manager<'id>;

/// Returns the `(value, down, right)` triple of the root node of `set`, or `None` if `set` is a terminal.
///
/// This is [LDDFunction::node] for an already-borrowed `manager`, which avoids locking the manager again.
fn node_of(manager: &LddManager<'_>, set: &LDDFunction) -> Option<(Value, LDDFunction, LDDFunction)> {
    match manager.get_node(set.as_edge(manager)) {
        Node::Terminal(_) => None,
        Node::Inner(node) => {
            let mut children = node.children();
            let down = children.next().expect("an LDD node has a down child");
            let right = children.next().expect("an LDD node has a right child");
            Some((
                *node.value(),
                LDDFunction::from_edge(manager, manager.clone_edge(down)),
                LDDFunction::from_edge(manager, manager.clone_edge(right)),
            ))
        }
    }
}

/// A table of LDD nodes that are read bottom-up, for example from a file, where
/// the level of a node is not yet known.
///
/// Indices 0 and 1 are the empty set and the empty vector respectively, and every
/// [RawLddTable::push]ed node obtains the next index. Nodes are only constructed by
/// [RawLddTable::build], at which point their level is known. A node can be shared
/// between LDDs whose vectors differ in length, and is then constructed once per level.
#[derive(Default)]
pub struct RawLddTable {
    /// The (value, down, right) triples of the inner nodes, starting at index 2.
    nodes: Vec<(Value, usize, usize)>,
    cache: FxHashMap<(usize, LevelNo), LDDFunction>,
}

impl RawLddTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of indices in the table, including the two terminals.
    pub(crate) fn len(&self) -> usize {
        self.nodes.len() + 2
    }

    /// Adds the node `(value, down, right)`, where `down` and `right` must be existing indices.
    pub fn push(&mut self, value: Value, down: usize, right: usize) -> Result<(), String> {
        if down >= self.len() {
            return Err(format!("Read invalid down ldd index {down}, length {}", self.len()));
        }

        if right >= self.len() {
            return Err(format!("Read invalid right ldd index {right}, length {}", self.len()));
        }

        self.nodes.push((value, down, right));
        Ok(())
    }

    /// Constructs the LDD at `index`, residing at level 0.
    pub fn build(&mut self, manager: &LDDManagerRef, index: usize) -> Result<LDDFunction, MercError> {
        if index >= self.len() {
            return Err(format!("Read invalid ldd index {index}, length {}", self.len()).into());
        }

        Ok(self.build_at(manager, 0, index)?)
    }

    /// Constructs the LDD at `index`, residing at `level`.
    fn build_at(&mut self, manager: &LDDManagerRef, level: LevelNo, index: usize) -> AllocResult<LDDFunction> {
        // Walk the `right` sibling chain iteratively instead of recursing on it.
        let mut chain = Vec::new();
        let mut current = index;
        let mut result = loop {
            match current {
                0 => break manager.with_manager_shared(LDDFunction::empty_set)?,
                1 => break manager.with_manager_shared(LDDFunction::empty_vector)?,
                _ => {
                    if let Some(cached) = self.cache.get(&(current, level)) {
                        break cached.clone();
                    }
                    chain.push(current);
                    current = self.nodes[current - 2].2;
                }
            }
        };

        for index in chain.into_iter().rev() {
            let (value, down, _) = self.nodes[index - 2];
            let down = self.build_at(manager, level + 1, down)?;
            result = manager.with_manager_shared(|m| LDDFunction::make_node(m, level, value, &down, &result))?;
            self.cache.insert((index, level), result.clone());
        }

        Ok(result)
    }
}

/// Restricts `set` to the vectors whose element at `level` equals `value`.
///
/// `level` must be less than the length of every vector in `set` (checked once, up front, in
/// debug builds); every caller derives it from a valid position in the state vector, so this
/// should never trip in practice.
pub fn fix_element(manager: &LDDManagerRef, set: &LDDFunction, level: usize, value: Value) -> AllocResult<LDDFunction> {
    debug_assert!(
        set.is_empty() || level < height(manager, set),
        "fix_element: level {level} is out of range for a set of height {}",
        height(manager, set)
    );
    fix_element_rec(manager, 0, set, level, value)
}

/// Recursive implementation of [fix_element] for a `set` that resides at `depth`.
fn fix_element_rec(
    manager: &LDDManagerRef,
    depth: LevelNo,
    set: &LDDFunction,
    level: usize,
    value: Value,
) -> AllocResult<LDDFunction> {
    if level == 0 {
        let mut current = set.clone();
        loop {
            match current.node() {
                None => return manager.with_manager_shared(LDDFunction::empty_set),
                Some((v, down, right)) => match v.cmp(&value) {
                    Ordering::Equal => {
                        let empty = manager.with_manager_shared(LDDFunction::empty_set)?;
                        return manager.with_manager_shared(|m| LDDFunction::make_node(m, depth, value, &down, &empty));
                    }
                    Ordering::Greater => return manager.with_manager_shared(LDDFunction::empty_set),
                    Ordering::Less => current = right,
                },
            }
        }
    } else {
        // Walk the `right` sibling chain iteratively instead of recursing on it.
        let mut kept = Vec::new();
        let mut current = set.clone();
        loop {
            match current.node() {
                // `set` ran out of levels before reaching `level` (it is ∅, or `level` was past the
                // end of its vectors): there is no element at `level` to match, so the result is ∅.
                None => break,
                Some((v, down, right)) => {
                    let new_down = fix_element_rec(manager, depth + 1, &down, level - 1, value)?;
                    if !new_down.is_empty() {
                        kept.push((v, new_down));
                    }
                    current = right;
                }
            }
        }

        let mut result = manager.with_manager_shared(LDDFunction::empty_set)?;
        for (v, new_down) in kept.into_iter().rev() {
            result = manager.with_manager_shared(|m| LDDFunction::make_node(m, depth, v, &new_down, &result))?;
        }
        Ok(result)
    }
}

/// Returns an LDD containing all elements of the given iterator over vectors.
pub fn from_iter<'a, I>(manager: &LDDManagerRef, iter: I) -> LDDFunction
where
    I: Iterator<Item = &'a Vec<Value>>,
{
    let mut result = manager
        .with_manager_shared(|m| LDDFunction::empty_set(m))
        .expect("Failed to create the empty set");

    for vector in iter {
        let single = manager
            .with_manager_shared(|m| LDDFunction::singleton(m, vector))
            .expect("Failed to create a singleton");
        result = result.union(&single).expect("Failed to compute the union");
    }

    result
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use oxidd::ManagerRef;
    use oxidd::ldd::LDDFunction;

    use merc_utilities::random_test;

    use crate::LDD_CACHE_CAPACITY;
    use crate::LDD_NODE_CAPACITY;
    use crate::LddLenCache;
    use crate::from_iter;
    use crate::ldd_len;
    use crate::random_vector_set;

    use super::fix_element;
    use super::merge;

    /// The number of vectors in `ldd`, which is small enough for every test in this module to be exact.
    fn count(ldd: &LDDFunction) -> usize {
        ldd_len(ldd, &mut LddLenCache::new())
            .exact()
            .expect("the count of a test set is exact") as usize
    }

    /// Cross-checks [`LDDFunction::intersect`] against a `HashSet` reference implementation.
    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_random_intersect() {
        random_test(100, |rng| {
            let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
            manager.reserve_levels(3);

            let a = random_vector_set(rng, 8, 3, 5);
            let b = random_vector_set(rng, 8, 3, 5);

            let ldd_a = from_iter(&manager, a.iter());
            let ldd_b = from_iter(&manager, b.iter());

            let result = ldd_a.intsec(&ldd_b).unwrap();
            let expected: HashSet<Vec<u32>> = a.intersection(&b).cloned().collect();

            assert_eq!(count(&result), expected.len());
            for vector in &expected {
                assert!(crate::element_of(&manager, vector, &result), "missing {vector:?}");
            }
        })
    }

    /// Cross-checks [merge] against a `HashSet` reference implementation of the
    /// interleaved cartesian product.
    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_random_merge() {
        random_test(100, |rng| {
            let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
            manager.reserve_levels(6);

            let a = random_vector_set(rng, 8, 3, 5);
            let b = random_vector_set(rng, 8, 3, 5);

            let ldd_a = from_iter(&manager, a.iter());
            let ldd_b = from_iter(&manager, b.iter());

            let result = merge(&manager, &ldd_a, &ldd_b).unwrap();

            let mut expected: HashSet<Vec<u32>> = HashSet::new();
            for va in &a {
                for vb in &b {
                    let mut interleaved = Vec::with_capacity(va.len() + vb.len());
                    for (x, y) in va.iter().zip(vb.iter()) {
                        interleaved.push(*x);
                        interleaved.push(*y);
                    }
                    expected.insert(interleaved);
                }
            }

            assert_eq!(count(&result), expected.len());
            for vector in &expected {
                assert!(crate::element_of(&manager, vector, &result), "missing {vector:?}");
            }
        })
    }

    /// Cross-checks [fix_element] against a `HashSet` reference implementation.
    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_random_fix_element() {
        random_test(100, |rng| {
            let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
            manager.reserve_levels(5);

            let length = 5;
            let max_value = 5;
            let set = random_vector_set(rng, 32, length, max_value);
            let ldd = from_iter(&manager, set.iter());

            for level in 0..length {
                for value in 0..max_value {
                    let result = fix_element(&manager, &ldd, level, value).unwrap();
                    let expected: HashSet<Vec<u32>> = set.iter().filter(|v| v[level] == value).cloned().collect();

                    assert_eq!(
                        count(&result),
                        expected.len(),
                        "level {level}, value {value}: {:?} vs {:?}",
                        count(&result),
                        expected.len()
                    );
                    for vector in &expected {
                        assert!(crate::element_of(&manager, vector, &result), "missing {vector:?}");
                    }
                }
            }
        })
    }

    /// [`fix_element`] on the empty set must return the empty set regardless of `level`, not the
    /// terminal it happens to bottom out on.
    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_fix_element_empty_set() {
        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let empty = manager.with_manager_shared(LDDFunction::empty_set).unwrap();

        for level in 0..5 {
            let result = fix_element(&manager, &empty, level, 0).unwrap();
            assert!(result.is_empty(), "level {level}: expected the empty set");
        }
    }
}
