#![forbid(unsafe_code)]

/// Traverses `root` in post-order, without recursion.
///
/// `expand` is called on every item that is visited, and lists, in order, what the item stands
/// for: items to visit in turn, with [Expansion::visit], and values to pass to `emit`, with
/// [Expansion::emit]. Everything that one call of `expand` lists is handled in that order, and
/// before anything that an earlier call listed after it. Listing the children of a node and then
/// the node itself therefore emits the nodes in post-order: for `f(g(a), b)` the order is `a`,
/// `g`, `b`, `f`.
pub(crate) fn post_order<T, E, X, O>(root: T, mut expand: X, mut emit: O)
where
    X: FnMut(T, &mut Expansion<'_, T, E>),
    O: FnMut(E),
{
    let mut work = vec![Item::Visit(root)];
    while let Some(item) = work.pop() {
        match item {
            Item::Visit(item) => {
                // The listed items are pushed in order, and reversed so that the first is on top.
                let first = work.len();
                expand(item, &mut Expansion { work: &mut work });
                work[first..].reverse();
            }
            Item::Emit(value) => emit(value),
        }
    }
}

/// What [post_order] still has to do.
enum Item<T, E> {
    Visit(T),
    Emit(E),
}

/// What an item stands for in [post_order].
pub(crate) struct Expansion<'a, T, E> {
    work: &'a mut Vec<Item<T, E>>,
}

impl<T, E> Expansion<'_, T, E> {
    /// Visits `item`, after what is listed before it.
    pub fn visit(&mut self, item: T) {
        self.work.push(Item::Visit(item));
    }

    /// Emits `value`, after what is listed before it.
    pub fn emit(&mut self, value: E) {
        self.work.push(Item::Emit(value));
    }
}

#[cfg(test)]
mod tests {
    use super::post_order;

    /// A node with a label and children.
    struct Node(&'static str, Vec<Node>);

    #[test]
    fn test_post_order_emits_children_before_their_parent() {
        let tree = Node("f", vec![Node("g", vec![Node("a", vec![])]), Node("b", vec![])]);

        let mut order = Vec::new();
        post_order(
            tree,
            |Node(label, children), expansion| {
                for child in children {
                    expansion.visit(child);
                }
                expansion.emit(label);
            },
            |label| order.push(label),
        );
        assert_eq!(order, ["a", "g", "b", "f"]);
    }
}
