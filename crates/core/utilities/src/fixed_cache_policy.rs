use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::Hash;

/// A trait for implementing cache replacement policies.
///
/// Cache policies determine which elements to remove when the cache is full.
pub trait CachePolicy<K>: Default {
    /// Called when the cache is cleared.
    fn clear(&mut self);

    /// Called when a new element is inserted into the cache.
    fn inserted(&mut self, key: K);

    /// Called when an element is accessed in the cache.
    fn touch(&mut self, key: &K);

    /// Returns the key that should be replaced when the cache is full.
    fn replacement_candidate<V>(&mut self, cache: &HashMap<K, V>) -> Option<K>;
}

/// A policy that replaces an arbitrary element.
#[derive(Debug, Default)]
pub struct NoPolicy;

impl<K: Clone> CachePolicy<K> for NoPolicy {
    fn clear(&mut self) {}
    fn inserted(&mut self, _key: K) {}
    fn touch(&mut self, _key: &K) {}

    fn replacement_candidate<V>(&mut self, cache: &HashMap<K, V>) -> Option<K> {
        cache.keys().next().cloned()
    }
}

/// A First-In-First-Out (FIFO) replacement policy.
#[derive(Debug)]
pub struct FifoPolicy<K> {
    queue: VecDeque<K>,
}

impl<K> FifoPolicy<K> {
    pub fn new() -> Self {
        Self { queue: VecDeque::new() }
    }
}

impl<K> Default for FifoPolicy<K> {
    fn default() -> Self {
        Self { queue: VecDeque::new() }
    }
}

impl<K: Clone + Eq + Hash> CachePolicy<K> for FifoPolicy<K> {
    fn clear(&mut self) {
        self.queue.clear();
    }

    fn inserted(&mut self, key: K) {
        self.queue.push_back(key);
    }

    fn touch(&mut self, _key: &K) {
        // FIFO policy doesn't use access information
    }

    fn replacement_candidate<V>(&mut self, cache: &HashMap<K, V>) -> Option<K> {
        while let Some(key) = self.queue.pop_front() {
            if cache.contains_key(&key) {
                return Some(key);
            }
        }
        None
    }
}

/// A Least-Recently-Used (LRU) replacement policy.
///
/// `touch` is linear in the number of resident keys, so this policy is only
/// suitable for small caches.
#[derive(Debug)]
pub struct LruPolicy<K> {
    queue: VecDeque<K>,
}

impl<K> LruPolicy<K> {
    pub fn new() -> Self {
        Self { queue: VecDeque::new() }
    }
}

impl<K> Default for LruPolicy<K> {
    fn default() -> Self {
        Self { queue: VecDeque::new() }
    }
}

impl<K: Clone + Eq + Hash> CachePolicy<K> for LruPolicy<K> {
    fn clear(&mut self) {
        self.queue.clear();
    }

    fn inserted(&mut self, key: K) {
        self.queue.push_back(key);
    }

    fn touch(&mut self, key: &K) {
        if let Some(pos) = self.queue.iter().position(|k| k == key)
            && let Some(k) = self.queue.remove(pos)
        {
            self.queue.push_back(k);
        }
    }

    fn replacement_candidate<V>(&mut self, cache: &HashMap<K, V>) -> Option<K> {
        while let Some(key) = self.queue.pop_front() {
            if cache.contains_key(&key) {
                return Some(key);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::CachePolicy;
    use super::FifoPolicy;
    use super::LruPolicy;
    use super::NoPolicy;

    #[test]
    fn test_no_policy() {
        let mut policy = NoPolicy;
        let mut cache = HashMap::new();
        cache.insert(1, "one");
        cache.insert(2, "two");

        assert!(policy.replacement_candidate(&cache).is_some());
    }

    #[test]
    fn test_fifo_policy() {
        let mut policy = FifoPolicy::default();
        let mut cache = HashMap::new();

        // Insert elements
        cache.insert(1, "one");
        policy.inserted(1);
        cache.insert(2, "two");
        policy.inserted(2);

        // First in should be first out
        assert_eq!(policy.replacement_candidate(&cache), Some(1));
    }

    #[test]
    fn test_lru_policy() {
        let mut policy = LruPolicy::default();
        let mut cache = HashMap::new();

        // Insert elements
        cache.insert(1, "one");
        policy.inserted(1);
        cache.insert(2, "two");
        policy.inserted(2);

        // Touch first element
        policy.touch(&1);

        // Least recently used should be 2
        assert_eq!(policy.replacement_candidate(&cache), Some(2));
    }
}
