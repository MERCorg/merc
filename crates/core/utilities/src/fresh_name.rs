use std::collections::HashSet;

/// Generates names of the form `<base><index>` that are distinct from a set of names already in
/// use, and from every name it generated before, whatever their base.
pub struct FreshNameGenerator {
    used: HashSet<String>,
}

impl FreshNameGenerator {
    /// Creates a generator that avoids every name in `used`.
    pub fn new<I>(used: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        FreshNameGenerator {
            used: used.into_iter().collect(),
        }
    }

    /// Returns `base` suffixed with the smallest natural number that makes it fresh, and marks
    /// the result as used.
    pub fn generate(&mut self, base: &str) -> String {
        let mut index = 0;
        loop {
            let candidate = format!("{base}{index}");
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
            index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FreshNameGenerator;

    #[test]
    fn test_generate_avoids_used_and_generated_names() {
        let mut generator = FreshNameGenerator::new(["X0".to_string()]);
        assert_eq!(generator.generate("X"), "X1");
        assert_eq!(generator.generate("X"), "X2");
        assert_eq!(generator.generate("Y"), "Y0");
    }
}
