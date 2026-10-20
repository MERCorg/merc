use merc_utilities::MercIndex;

/// A general trait for graph structures.
pub trait Graph {
    /// The index type for the vertices.
    type VertexIndex: MercIndex;

    /// The index type for the edge labels.
    type LabelIndex: MercIndex;

    /// Returns the number of vertices in the graph.
    fn num_of_vertices(&self) -> usize;

    /// Iterate over all the vertices in the graph.
    fn iter_vertices(&self) -> impl Iterator<Item = Self::VertexIndex>;

    /// Returns an iterator over all the outgoing edges of a given vertex.
    fn outgoing_edges(&self, vertex: Self::VertexIndex) -> impl Iterator<Item = (Self::LabelIndex, Self::VertexIndex)>;
}
