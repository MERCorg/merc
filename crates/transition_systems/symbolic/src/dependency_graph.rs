use std::fmt;

use log::trace;

/// Represents a dependency graph between variables used in symbolic transition relations.
pub struct DependencyGraph {
    /// The list of relations in the dependency graph.
    relations: Vec<Relation>,

    /// The number of vertices
    num_of_vertices: usize,
}

impl DependencyGraph {
    /// Creates a new dependency graph from the given relations.
    pub fn new(relations: Vec<Relation>) -> Self {
        let num_of_vertices = relations
            .iter()
            .flat_map(|rel| rel.read_vars.iter().chain(rel.write_vars.iter()))
            .copied()
            .max()
            .map_or(0, |max_index| max_index + 1);

        DependencyGraph {
            relations,
            num_of_vertices,
        }
    }

    /// Returns the graph restricted to the vertices in `order`, renumbered to their position in it.
    ///
    /// A relation is dropped once none of its read or write variables remain after restriction.
    pub fn reorder(&self, order: &[usize]) -> Self {
        let mut new_relations = Vec::with_capacity(self.relations.len());

        for relation in &self.relations {
            let mut new_read_vars: Vec<usize> = relation
                .read_vars()
                .filter_map(|var| order.iter().position(|&v| v == var))
                .collect();

            let mut new_write_vars: Vec<usize> = relation
                .write_vars()
                .filter_map(|var| order.iter().position(|&v| v == var))
                .collect();

            new_read_vars.sort_unstable();
            new_write_vars.sort_unstable();

            if !new_read_vars.is_empty() || !new_write_vars.is_empty() {
                new_relations.push(Relation::new(new_read_vars, new_write_vars));
            }
        }

        DependencyGraph::new(new_relations)
    }

    /// Returns the number of vertices in the dependency graph.
    pub fn num_of_vertices(&self) -> usize {
        self.num_of_vertices
    }

    /// Number of relations in the dependency graph.
    pub fn num_of_relations(&self) -> usize {
        self.relations.len()
    }

    /// Returns an iterator over the relations in the dependency graph.
    pub fn relations(&self) -> impl Iterator<Item = &Relation> {
        self.relations.iter()
    }

    /// Returns the total span of all relations in the dependency graph.
    pub fn total_span(&self) -> usize {
        self.relations.iter().map(|rel| rel.span()).sum()
    }
}

impl fmt::Debug for DependencyGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "graph with {} vertices, and {} hyper-edges",
            self.num_of_vertices,
            self.relations.len()
        )?;
        for (i, relation) in self.relations.iter().enumerate() {
            writeln!(f, "  {}: {:?}", i, relation)?;
        }
        Ok(())
    }
}

/// A single relation in the dependency graph containing read and write
/// dependencies onto variables, given by their indices.
pub struct Relation {
    read_vars: Vec<usize>,
    write_vars: Vec<usize>,
}

impl Relation {
    /// Create a new relation or hyper-edge.
    pub fn new(read_vars: Vec<usize>, write_vars: Vec<usize>) -> Self {
        Relation { read_vars, write_vars }
    }

    /// Returns an iterator over the read variables in this relation.
    pub fn read_vars(&self) -> impl Iterator<Item = usize> + '_ {
        self.read_vars.iter().copied()
    }

    /// Returns an iterator over the write variables in this relation.
    pub fn write_vars(&self) -> impl Iterator<Item = usize> + '_ {
        self.write_vars.iter().copied()
    }

    /// Returns the topmost (lowest-index) variable this relation reads or writes, or `None` if it
    /// reads and writes nothing.
    pub fn top(&self) -> Option<usize> {
        let read = self.read_vars.iter().min().copied();
        let write = self.write_vars.iter().min().copied();
        read.into_iter().chain(write).min()
    }

    /// Returns the bottommost (highest-index) variable this relation reads or writes, or `None` if
    /// it reads and writes nothing.
    pub fn bot(&self) -> Option<usize> {
        let read = self.read_vars.iter().max().copied();
        let write = self.write_vars.iter().max().copied();
        read.into_iter().chain(write).max()
    }

    /// Returns the span of the relation, i.e., the range between the minimum and maximum
    /// variable indices used by this relation. Zero for a relation that reads and writes nothing.
    pub fn span(&self) -> usize {
        match (self.top(), self.bot()) {
            (Some(top), Some(bot)) => bot - top + 1,
            _ => 0,
        }
    }
}

impl fmt::Debug for Relation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} -> {:?}", self.read_vars, self.write_vars)
    }
}

/// Parses a dependency graph as output by the `--info` option of both
/// [lpreach](https://mcrl2.org/web/user_manual/tools/release/lpsreach.html) and
/// [pbessolvesymbolic](https://mcrl2.org/web/user_manual/tools/release/pbessolvesymbolic.html).
pub fn parse_compacted_dependency_graph(input: &str) -> DependencyGraph {
    trace!("Parsing dependency graph:\n{}", input);
    let mut relations = Vec::new();

    for line in input.lines() {
        if let Some(relation) = parse_pattern_line(line) {
            relations.push(relation);
        }
    }

    DependencyGraph::new(relations)
}

/// Parses a single line of a compacted dependency graph into a [`Relation`],
/// returning `None` for lines that carry no pattern characters.
fn parse_pattern_line(line: &str) -> Option<Relation> {
    if line == "read/write patterns compacted" {
        return None;
    }

    // Keep only pattern characters, ignoring indices/whitespace
    let pattern: Vec<char> = line.chars().filter(|c| matches!(c, '+' | '-' | 'r' | 'w')).collect();

    if pattern.is_empty() {
        return None;
    }

    let mut read_vars = Vec::new();
    let mut write_vars = Vec::new();

    for (col, ch) in pattern.into_iter().enumerate() {
        match ch {
            '+' => {
                read_vars.push(col);
                write_vars.push(col);
            }
            'r' => read_vars.push(col),
            'w' => write_vars.push(col),
            '-' => {}
            _ => {}
        }
    }

    Some(Relation { read_vars, write_vars })
}

#[cfg(test)]
mod tests {
    use crate::Relation;
    use crate::parse_compacted_dependency_graph;

    #[test]
    fn test_parse_abp_dependency_graph() {
        let input = "1 +w---------
2 ---+++-----
3 ------++---
4 --------++-
5 ------+w+w+
6 ---+ww--+w-
7 ---+++--+wr
8 +-----+w---
9 +rr+ww-----
10 +++---++---";

        let graph = parse_compacted_dependency_graph(input);

        assert_eq!(graph.relations.len(), 10);
    }

    #[test]
    fn test_relation_top_bot_read_only() {
        let relation = Relation::new(vec![2, 5], vec![]);
        assert_eq!(relation.top(), Some(2));
        assert_eq!(relation.bot(), Some(5));
        assert_eq!(relation.span(), 4);
    }

    #[test]
    fn test_relation_top_bot_write_only() {
        let relation = Relation::new(vec![], vec![1, 4]);
        assert_eq!(relation.top(), Some(1));
        assert_eq!(relation.bot(), Some(4));
        assert_eq!(relation.span(), 4);
    }

    #[test]
    fn test_relation_top_bot_mixed() {
        // Read and write ranges overlap: top/bot must consider both.
        let relation = Relation::new(vec![1, 3], vec![2, 3]);
        assert_eq!(relation.top(), Some(1));
        assert_eq!(relation.bot(), Some(3));
        assert_eq!(relation.span(), 3);
    }

    #[test]
    fn test_relation_top_bot_disjoint() {
        // Reads and writes at opposite ends of the vector: span covers everything in between.
        let relation = Relation::new(vec![0], vec![9]);
        assert_eq!(relation.top(), Some(0));
        assert_eq!(relation.bot(), Some(9));
        assert_eq!(relation.span(), 10);
    }

    #[test]
    fn test_relation_top_bot_empty() {
        // A degenerate group with no reads or writes has no well-defined top/bot.
        let relation = Relation::new(vec![], vec![]);
        assert_eq!(relation.top(), None);
        assert_eq!(relation.bot(), None);
        assert_eq!(relation.span(), 0);
    }
}
