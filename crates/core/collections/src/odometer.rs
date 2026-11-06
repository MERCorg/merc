//! A mixed-radix counter that enumerates a Cartesian product of index ranges.

/// Enumerates every combination of digits `d` with `d[k] < radices[k]`, in which the last digit changes fastest.
///
/// Unlike [`itertools::Itertools::multi_cartesian_product`], the combination is updated in place, and every step reports
/// the first digit that changed, so that a caller can update only what depends on the changed digits.
#[derive(Clone, Debug, Default)]
pub struct Odometer {
    digits: Vec<usize>,
    radices: Vec<usize>,
}

impl Odometer {
    /// Creates an odometer with no digits, which has exactly one (empty) combination.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resets the odometer to the all-zero combination for the given radices, reusing the allocated buffers.
    ///
    /// Returns false iff a radix is zero, in which case there are no combinations.
    pub fn reset(&mut self, radices: impl IntoIterator<Item = usize>) -> bool {
        self.radices.clear();
        self.radices.extend(radices);
        self.digits.clear();
        self.digits.resize(self.radices.len(), 0);
        !self.radices.contains(&0)
    }

    /// Returns the current combination.
    pub fn digits(&self) -> &[usize] {
        &self.digits
    }

    /// Advances to the next combination, and returns the first digit that changed. All digits after it are reset to
    /// zero. Returns `None` after the last combination, at which point the odometer is back at all zeros.
    pub fn increment(&mut self) -> Option<usize> {
        for k in (0..self.digits.len()).rev() {
            self.digits[k] += 1;
            if self.digits[k] < self.radices[k] {
                return Some(k);
            }
            self.digits[k] = 0;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;

    use super::Odometer;

    /// Collects every combination of the odometer, with the first changed digit of every step.
    fn combinations(radices: &[usize]) -> (Vec<Vec<usize>>, Vec<usize>) {
        let mut odometer = Odometer::new();
        if !odometer.reset(radices.iter().copied()) {
            return (Vec::new(), Vec::new());
        }

        let mut result = vec![odometer.digits().to_vec()];
        let mut changed = Vec::new();
        while let Some(first) = odometer.increment() {
            let previous = result.last().unwrap();
            assert!(
                previous[..first] == odometer.digits()[..first] && previous[first] != odometer.digits()[first],
                "Digit {first} must be the first digit that changed"
            );
            result.push(odometer.digits().to_vec());
            changed.push(first);
        }
        assert!(odometer.digits().iter().all(|&digit| digit == 0));
        (result, changed)
    }

    #[test]
    fn test_odometer_matches_cartesian_product() {
        for radices in [vec![], vec![1], vec![3], vec![2, 3], vec![3, 1, 2], vec![2, 0, 2]] {
            let expected: Vec<Vec<usize>> = radices
                .iter()
                .map(|&radix| 0..radix)
                .multi_cartesian_product()
                .collect();
            let (result, _) = combinations(&radices);
            if radices.is_empty() {
                // multi_cartesian_product yields nothing for no iterators, but there is one empty combination.
                assert_eq!(result, vec![Vec::<usize>::new()]);
            } else {
                assert_eq!(result, expected, "radices {radices:?}");
            }
        }
    }

    #[test]
    fn test_odometer_reports_first_changed_digit() {
        let (_, changed) = combinations(&[2, 2]);
        assert_eq!(changed, vec![1, 0, 1]);
    }
}
