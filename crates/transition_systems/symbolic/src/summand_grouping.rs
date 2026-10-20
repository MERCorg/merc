use std::fmt;
use std::str::FromStr;

use merc_utilities::MercError;

/// The read/write pattern of a single summand over the positions of the state vector.
///
/// Mirrors the `boost::dynamic_bitset` patterns of the mCRL2 `mcrl2::symbolic` library, where bit
/// `2i` records that position `i` is read and bit `2i+1` that it is written. Here both halves are
/// kept as separate bit vectors, but they carry the same information.
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReadWritePattern {
    /// `read[i]` is true iff position `i` of the state vector is read.
    read: Vec<bool>,

    /// `write[i]` is true iff position `i` of the state vector is written.
    write: Vec<bool>,
}

impl ReadWritePattern {
    /// Creates the empty pattern over a state vector of `num_positions` positions.
    pub fn new(num_positions: usize) -> Self {
        ReadWritePattern {
            read: vec![false; num_positions],
            write: vec![false; num_positions],
        }
    }

    /// Creates the pattern reading `read` and writing `write`.
    ///
    /// Fails when an index is not a position of a state vector of length `num_positions`.
    pub fn from_indices(num_positions: usize, read: &[usize], write: &[usize]) -> Result<Self, MercError> {
        let mut pattern = ReadWritePattern::new(num_positions);
        for &position in read {
            pattern.set_read(position)?;
        }
        for &position in write {
            pattern.set_write(position)?;
        }
        Ok(pattern)
    }

    /// Returns the number of state vector positions this pattern is defined over.
    pub fn num_positions(&self) -> usize {
        self.read.len()
    }

    /// Returns whether position `i` is read.
    pub fn reads(&self, i: usize) -> bool {
        self.read[i]
    }

    /// Returns whether position `i` is written.
    pub fn writes(&self, i: usize) -> bool {
        self.write[i]
    }

    /// Returns whether position `i` is used, i.e. either read or written.
    pub fn uses(&self, i: usize) -> bool {
        self.reads(i) || self.writes(i)
    }

    /// Marks position `i` as read, failing when it is out of range.
    pub fn set_read(&mut self, i: usize) -> Result<(), MercError> {
        let num_positions = self.num_positions();
        let read = self
            .read
            .get_mut(i)
            .ok_or_else(|| format!("read position {i} is out of range for {num_positions} position(s)"))?;
        *read = true;
        Ok(())
    }

    /// Marks position `i` as written, failing when it is out of range.
    pub fn set_write(&mut self, i: usize) -> Result<(), MercError> {
        let num_positions = self.num_positions();
        let write = self
            .write
            .get_mut(i)
            .ok_or_else(|| format!("write position {i} is out of range for {num_positions} position(s)"))?;
        *write = true;
        Ok(())
    }

    /// Returns the read positions in increasing order.
    pub fn read_positions(&self) -> impl Iterator<Item = usize> + '_ {
        self.read.iter().enumerate().filter(|(_, r)| **r).map(|(i, _)| i)
    }

    /// Returns the written positions in increasing order.
    pub fn write_positions(&self) -> impl Iterator<Item = usize> + '_ {
        self.write.iter().enumerate().filter(|(_, w)| **w).map(|(i, _)| i)
    }

    /// Returns the pattern that only records _whether_ a position is used, by marking every used
    /// position as both read and written.
    ///
    /// This is the key of the [`SummandGrouping::Used`] strategy: two summands land in the same
    /// group exactly when they use the same positions, regardless of how they use them.
    pub fn used(&self) -> Self {
        let used: Vec<bool> = (0..self.num_positions()).map(|i| self.uses(i)).collect();
        ReadWritePattern {
            read: used.clone(),
            write: used,
        }
    }

    /// Returns this pattern with its positions permuted by `order`, i.e. position `order[i]` of this
    /// pattern becomes position `i` of the result.
    ///
    /// Applying the variable order to the matrix shows it the way the decision diagram stores it, as
    /// mCRL2's `reorder_read_write_patterns` does. Fails when `order` is not a permutation of the
    /// positions of this pattern.
    pub fn permute(&self, order: &[usize]) -> Result<Self, MercError> {
        if order.len() != self.num_positions() {
            return Err(format!(
                "cannot permute a read/write pattern over {} position(s) with an order of {} position(s)",
                self.num_positions(),
                order.len()
            )
            .into());
        }

        let mut result = ReadWritePattern::new(self.num_positions());
        for (level, &position) in order.iter().enumerate() {
            if *self
                .read
                .get(position)
                .ok_or_else(|| format!("position {position} is out of range for the variable order"))?
            {
                result.set_read(level)?;
            }
            if self.write[position] {
                result.set_write(level)?;
            }
        }
        Ok(result)
    }

    /// Returns the pointwise union of two patterns.
    ///
    /// Fails when the patterns are defined over state vectors of different lengths.
    pub fn union(&self, other: &Self) -> Result<Self, MercError> {
        if self.num_positions() != other.num_positions() {
            return Err(format!(
                "cannot combine read/write patterns over {} and {} position(s)",
                self.num_positions(),
                other.num_positions()
            )
            .into());
        }

        Ok(ReadWritePattern {
            read: self.read.iter().zip(&other.read).map(|(a, b)| *a || *b).collect(),
            write: self.write.iter().zip(&other.write).map(|(a, b)| *a || *b).collect(),
        })
    }
}

impl fmt::Display for ReadWritePattern {
    /// Prints the pattern compactly as one character per position, matching the `print_read_write_patterns`
    /// output of mCRL2: `+` for read and written, `r` for read, `w` for written and `-` for unused.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for i in 0..self.num_positions() {
            let character = match (self.reads(i), self.writes(i)) {
                (true, true) => '+',
                (true, false) => 'r',
                (false, true) => 'w',
                (false, false) => '-',
            };
            write!(f, "{character}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for ReadWritePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

/// How the summands of a linear process are distributed over the transition groups of symbolic
/// reachability.
///
/// Mirrors the `--groups` option of the mCRL2 `lpsreach` and `pbessolvesymbolic` tools. The grouping
/// does not change the reachable set, only the shape (and hence the size) of the symbolic transition
/// relations: fewer, larger groups mean fewer relational products per iteration, but each relation is
/// defined over more state vector positions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SummandGrouping {
    /// Every summand gets its own transition group.
    None,

    /// Joins the summands that use (read or write) exactly the same parameters.
    Used,

    /// Joins the summands with exactly the same read/write pattern.
    #[default]
    Simple,

    /// A user defined partition of the summand indices, e.g. `0; 1 3 4; 2 5`.
    Custom(Vec<Vec<usize>>),
}

impl SummandGrouping {
    /// Computes the transition groups for the summands with the given read/write `patterns`.
    ///
    /// The result is a partition of `0..patterns.len()`; every group is sorted and non-empty. Fails
    /// when a [`SummandGrouping::Custom`] grouping is not such a partition.
    pub fn compute(&self, patterns: &[ReadWritePattern]) -> Result<Vec<Vec<usize>>, MercError> {
        match self {
            SummandGrouping::None => Ok((0..patterns.len()).map(|i| vec![i]).collect()),
            SummandGrouping::Used => Ok(group_by_key(patterns, ReadWritePattern::used)),
            SummandGrouping::Simple => Ok(group_by_key(patterns, Clone::clone)),
            SummandGrouping::Custom(groups) => {
                validate_partition(groups, patterns.len())?;

                // A `Custom` grouping can also be built directly, so sort here rather than relying on
                // the parser having done it.
                let mut groups = groups.clone();
                for group in &mut groups {
                    group.sort_unstable();
                }
                Ok(groups)
            }
        }
    }
}

impl FromStr for SummandGrouping {
    type Err = MercError;

    /// Parses the mCRL2 `--groups` argument: one of the named strategies, or a user defined list of
    /// groups separated by semicolons, e.g. `0; 1 3 4; 2 5`.
    fn from_str(text: &str) -> Result<Self, MercError> {
        match text.trim() {
            "none" => Ok(SummandGrouping::None),
            "used" => Ok(SummandGrouping::Used),
            "simple" => Ok(SummandGrouping::Simple),
            groups => {
                let mut result = Vec::new();
                for part in groups.split(';') {
                    let mut group = part
                        .split_whitespace()
                        .map(|index| {
                            index.parse::<usize>().map_err(|_| {
                                MercError::from(format!("'{index}' in the groups '{text}' is not an index"))
                            })
                        })
                        .collect::<Result<Vec<usize>, MercError>>()?;

                    if group.is_empty() {
                        return Err(format!("the groups '{text}' contain an empty group").into());
                    }
                    group.sort_unstable();
                    result.push(group);
                }

                Ok(SummandGrouping::Custom(result))
            }
        }
    }
}

impl fmt::Display for SummandGrouping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SummandGrouping::None => write!(f, "none"),
            SummandGrouping::Used => write!(f, "used"),
            SummandGrouping::Simple => write!(f, "simple"),
            SummandGrouping::Custom(groups) => {
                for (i, group) in groups.iter().enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    for (j, index) in group.iter().enumerate() {
                        if j > 0 {
                            write!(f, " ")?;
                        }
                        write!(f, "{index}")?;
                    }
                }
                Ok(())
            }
        }
    }
}

/// Returns the read/write pattern of the transition group formed by the summands in `group`.
///
/// This is the union of the patterns of its members, closed off with the reads that make the merged
/// group faithful: a position written by the group, but neither read nor written by one of its
/// members, is added to the reads. Such a member leaves the position untouched, so its transitions
/// must repeat the source value on the write side of the group's shared short vector, which is only
/// possible when that value is part of the group's read projection. (mCRL2 instead marks these
/// positions as `copy` in the summand's cube, which its `union_cube_copy` interprets; the relations
/// here have no copy nodes, so the value is read instead.)
///
/// Note that the [`SummandGrouping::Used`] and [`SummandGrouping::Simple`] strategies never need
/// these extra reads, since their members agree on the positions they use.
///
/// Fails when `group` contains an index that is not a summand of `patterns`, or when the patterns are
/// defined over state vectors of different lengths.
pub fn transition_group_pattern(patterns: &[ReadWritePattern], group: &[usize]) -> Result<ReadWritePattern, MercError> {
    let num_positions = patterns.first().map_or(0, ReadWritePattern::num_positions);

    let mut result = ReadWritePattern::new(num_positions);
    for &index in group {
        let pattern = patterns.get(index).ok_or_else(|| {
            format!(
                "summand {index} does not exist, there are {} summand(s)",
                patterns.len()
            )
        })?;
        result = result.union(pattern)?;
    }

    for position in 0..num_positions {
        if result.writes(position) && group.iter().any(|&index| !patterns[index].uses(position)) {
            result.set_read(position)?;
        }
    }

    Ok(result)
}

/// Prints the read/write matrix compactly, one numbered line per pattern, as in mCRL2's
/// `print_read_write_patterns`.
pub fn print_read_write_patterns(patterns: &[ReadWritePattern]) -> String {
    use std::fmt::Write;

    let mut result = String::new();
    for (i, pattern) in patterns.iter().enumerate() {
        writeln!(result, "{i:>4} {pattern}").expect("writing to a String cannot fail");
    }
    result
}

/// Prints the read/write matrix of the transition groups, one numbered line per group, followed by
/// the summands it contains.
///
/// The `group_patterns` are the patterns of the groups, as returned by [transition_group_pattern];
/// the surplus of either argument is ignored.
pub fn print_transition_groups(groups: &[Vec<usize>], group_patterns: &[ReadWritePattern]) -> String {
    use std::fmt::Write;

    let mut result = String::new();
    for (i, (group, pattern)) in groups.iter().zip(group_patterns).enumerate() {
        writeln!(result, "{i:>4} {pattern} {group:?}").expect("writing to a String cannot fail");
    }
    result
}

/// Groups the indices of `patterns` by the given key, keeping the groups in the order in which their
/// key was first encountered.
fn group_by_key<K: PartialEq, F>(patterns: &[ReadWritePattern], key: F) -> Vec<Vec<usize>>
where
    F: Fn(&ReadWritePattern) -> K,
{
    let mut keys: Vec<K> = Vec::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();

    for (index, pattern) in patterns.iter().enumerate() {
        let pattern_key = key(pattern);
        match keys.iter().position(|existing| *existing == pattern_key) {
            Some(group) => groups[group].push(index),
            None => {
                keys.push(pattern_key);
                groups.push(vec![index]);
            }
        }
    }

    groups
}

/// Checks that `groups` is a partition of `0..num_summands`.
fn validate_partition(groups: &[Vec<usize>], num_summands: usize) -> Result<(), MercError> {
    let mut seen = vec![false; num_summands];

    for group in groups {
        if group.is_empty() {
            return Err("a transition group cannot be empty".into());
        }

        for &index in group {
            let assigned = seen
                .get_mut(index)
                .ok_or_else(|| format!("group index {index} does not exist, there are {num_summands} summand(s)"))?;
            if *assigned {
                return Err(format!("summand {index} occurs in more than one group").into());
            }
            *assigned = true;
        }
    }

    if let Some(missing) = seen.iter().position(|assigned| !assigned) {
        return Err(format!("summand {missing} does not occur in any group").into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The patterns of four summands over three positions, where summands 0 and 3 have the same
    /// pattern, and summand 1 uses the same positions as summand 0 with the roles swapped.
    fn patterns() -> Vec<ReadWritePattern> {
        vec![
            ReadWritePattern::from_indices(3, &[0], &[1]).unwrap(),
            ReadWritePattern::from_indices(3, &[1], &[0]).unwrap(),
            ReadWritePattern::from_indices(3, &[2], &[2]).unwrap(),
            ReadWritePattern::from_indices(3, &[0], &[1]).unwrap(),
        ]
    }

    #[test]
    fn test_pattern_display() {
        assert_eq!(patterns()[0].to_string(), "rw-");
        assert_eq!(patterns()[2].to_string(), "--+");
        assert_eq!(ReadWritePattern::new(3).to_string(), "---");
    }

    #[test]
    fn test_pattern_positions() {
        let pattern = ReadWritePattern::from_indices(4, &[3, 0], &[1]).unwrap();
        assert_eq!(pattern.read_positions().collect::<Vec<_>>(), vec![0, 3]);
        assert_eq!(pattern.write_positions().collect::<Vec<_>>(), vec![1]);
        assert_eq!(pattern.used().to_string(), "++-+");
    }

    #[test]
    fn test_pattern_permute() {
        // `rw-` with position 1 first becomes `wr-`.
        let pattern = patterns()[0].permute(&[1, 0, 2]).unwrap();
        assert_eq!(pattern.to_string(), "wr-");
        assert_eq!(patterns()[0].permute(&[0, 1, 2]).unwrap(), patterns()[0]);

        assert!(patterns()[0].permute(&[0, 1]).is_err());
        assert!(patterns()[0].permute(&[0, 1, 3]).is_err());
    }

    #[test]
    fn test_pattern_out_of_range() {
        assert!(ReadWritePattern::from_indices(2, &[2], &[]).is_err());
        assert!(ReadWritePattern::from_indices(2, &[], &[7]).is_err());
        assert!(ReadWritePattern::new(2).union(&ReadWritePattern::new(3)).is_err());
    }

    #[test]
    fn test_grouping_none() {
        let groups = SummandGrouping::None.compute(&patterns()).unwrap();
        assert_eq!(groups, vec![vec![0], vec![1], vec![2], vec![3]]);
    }

    #[test]
    fn test_grouping_simple() {
        // Only the summands with the identical pattern `rw-` are joined.
        let groups = SummandGrouping::Simple.compute(&patterns()).unwrap();
        assert_eq!(groups, vec![vec![0, 3], vec![1], vec![2]]);
    }

    #[test]
    fn test_grouping_used() {
        // Summand 1 uses the same positions as 0 and 3, so it joins them as well.
        let groups = SummandGrouping::Used.compute(&patterns()).unwrap();
        assert_eq!(groups, vec![vec![0, 1, 3], vec![2]]);
    }

    #[test]
    fn test_grouping_empty() {
        assert!(SummandGrouping::Used.compute(&[]).unwrap().is_empty());
        assert!(SummandGrouping::Simple.compute(&[]).unwrap().is_empty());
        assert!(SummandGrouping::None.compute(&[]).unwrap().is_empty());
    }

    #[test]
    fn test_grouping_parse() {
        assert_eq!("none".parse::<SummandGrouping>().unwrap(), SummandGrouping::None);
        assert_eq!("used".parse::<SummandGrouping>().unwrap(), SummandGrouping::Used);
        assert_eq!(" simple ".parse::<SummandGrouping>().unwrap(), SummandGrouping::Simple);
        assert_eq!(
            "0; 1 3 4; 2 5".parse::<SummandGrouping>().unwrap(),
            SummandGrouping::Custom(vec![vec![0], vec![1, 3, 4], vec![2, 5]])
        );

        assert!("0; ; 1".parse::<SummandGrouping>().is_err());
        assert!("0; a".parse::<SummandGrouping>().is_err());
        assert!("0; -1".parse::<SummandGrouping>().is_err());
    }

    #[test]
    fn test_grouping_display_roundtrip() {
        for text in ["none", "used", "simple", "0; 1 3 4; 2 5"] {
            let grouping: SummandGrouping = text.parse().unwrap();
            assert_eq!(grouping.to_string(), text);
        }
    }

    #[test]
    fn test_grouping_custom_must_be_a_partition() {
        let patterns = patterns();

        let grouping: SummandGrouping = "0 3; 1 2".parse().unwrap();
        assert_eq!(grouping.compute(&patterns).unwrap(), vec![vec![0, 3], vec![1, 2]]);

        // Groups built directly are sorted, and empty groups are rejected.
        let grouping = SummandGrouping::Custom(vec![vec![3, 0], vec![2, 1]]);
        assert_eq!(grouping.compute(&patterns).unwrap(), vec![vec![0, 3], vec![1, 2]]);
        assert!(
            SummandGrouping::Custom(vec![vec![0, 1, 2, 3], vec![]])
                .compute(&patterns)
                .is_err()
        );

        // Not covering every summand, overlapping groups and out of range indices are all rejected.
        assert!("0; 1".parse::<SummandGrouping>().unwrap().compute(&patterns).is_err());
        assert!(
            "0 1; 1 2 3"
                .parse::<SummandGrouping>()
                .unwrap()
                .compute(&patterns)
                .is_err()
        );
        assert!(
            "0 1 2 3 4"
                .parse::<SummandGrouping>()
                .unwrap()
                .compute(&patterns)
                .is_err()
        );
    }

    #[test]
    fn test_transition_group_pattern() {
        let patterns = patterns();

        // The union of `rw-` and `wr-` uses both positions in both roles.
        assert_eq!(transition_group_pattern(&patterns, &[0, 1]).unwrap().to_string(), "++-");
        assert_eq!(transition_group_pattern(&patterns, &[0, 3]).unwrap().to_string(), "rw-");
        assert_eq!(transition_group_pattern(&patterns, &[]).unwrap().to_string(), "---");

        // Summand 2 does not use position 1, which summand 0 writes, so the group reads it to be able
        // to repeat its value.
        assert_eq!(transition_group_pattern(&patterns, &[0, 2]).unwrap().to_string(), "r++");

        assert!(transition_group_pattern(&patterns, &[4]).is_err());
    }

    #[test]
    fn test_print_read_write_patterns() {
        let printed = print_read_write_patterns(&patterns());
        assert!(printed.contains("   0 rw-"), "{printed}");
        assert!(printed.contains("   2 --+"), "{printed}");
    }

    #[test]
    fn test_print_transition_groups() {
        let patterns = patterns();
        let groups = SummandGrouping::Used.compute(&patterns).unwrap();
        let group_patterns: Vec<ReadWritePattern> = groups
            .iter()
            .map(|group| transition_group_pattern(&patterns, group).unwrap())
            .collect();

        let printed = print_transition_groups(&groups, &group_patterns);
        assert!(printed.contains("   0 ++- [0, 1, 3]"), "{printed}");
        assert!(printed.contains("   1 --+ [2]"), "{printed}");
    }
}
