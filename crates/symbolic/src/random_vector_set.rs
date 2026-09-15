use oxidd::ldd::Value;
use rand::Rng;
use rand::RngExt;
use std::collections::HashSet;

/// Returns a vector of the given length with random u64 values (from 0..max_value).
pub fn random_vector<R: Rng>(rng: &mut R, length: usize, max_value: Value) -> Vec<Value> {
    let mut vector: Vec<Value> = Vec::new();
    for _ in 0..length {
        vector.push(rng.random_range(0..max_value));
    }

    vector
}

/// Returns a set of 'amount' vectors where every vector has the given length.
pub fn random_vector_set<R: Rng>(rng: &mut R, amount: usize, length: usize, max_value: Value) -> HashSet<Vec<Value>> {
    let mut result: HashSet<Vec<Value>> = HashSet::new();

    // Insert 'amount' number of vectors into the result.
    for _ in 0..amount {
        result.insert(random_vector(rng, length, max_value));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::random_vector;
    use super::random_vector_set;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn random_vector_has_requested_length() {
        for length in [0, 1, 16] {
            let mut rng = StdRng::seed_from_u64(42);
            let vector = random_vector(&mut rng, length, 3);
            assert_eq!(vector.len(), length);
        }
    }

    #[test]
    fn random_vector_values_stay_in_range() {
        let mut rng = StdRng::seed_from_u64(7);
        assert!(random_vector(&mut rng, 100, 1).iter().all(|&value| value < 1));
        assert!(random_vector(&mut rng, 100, 5).iter().all(|&value| value < 5));
    }

    #[test]
    fn random_vector_set_is_deduplicated() {
        let mut rng = StdRng::seed_from_u64(1);
        // Only three distinct vectors exist (length 1 over values 0..3), so
        // asking for more than that yields at most three.
        let set = random_vector_set(&mut rng, 100, 1, 3);
        assert!(set.len() <= 3);
        assert!(set.iter().all(|vector| vector.len() == 1));
    }

    #[test]
    fn random_vector_set_respects_length() {
        let mut rng = StdRng::seed_from_u64(2);
        let set = random_vector_set(&mut rng, 50, 4, 3);
        assert!(set.len() <= 50);
        assert!(set.iter().all(|vector| vector.len() == 4));
    }
}
