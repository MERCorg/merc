/// Tracks the bound variables in scope while traversing a term, and resolves
/// occurrences to their de Bruijn index.
///
/// Generic over the variable type `V` so that it is not tied to mCRL2 terms; the
/// symmetry detection graph instantiates it with `DataVariable`, which compares
/// on both name and sort.
pub struct BindingScope<V> {
    /// The variables in scope, innermost last.
    bound: Vec<V>,
}

impl<V> Default for BindingScope<V> {
    fn default() -> Self {
        BindingScope { bound: Vec::new() }
    }
}

impl<V: PartialEq> BindingScope<V> {
    /// Creates a scope without any bound variables.
    pub fn new() -> Self {
        Self::default()
    }

    /// Brings `variables` into scope, in list order, and returns how many were
    /// bound so that the caller can undo it with [`Self::pop`].
    pub fn push<I>(&mut self, variables: I) -> usize
    where
        I: IntoIterator<Item = V>,
    {
        let before = self.bound.len();
        self.bound.extend(variables);
        self.bound.len() - before
    }

    /// Removes the `count` innermost bound variables from scope.
    pub fn pop(&mut self, count: usize) {
        debug_assert!(count <= self.bound.len(), "cannot pop more variables than are bound");
        self.bound.truncate(self.bound.len() - count);
    }

    /// Returns the de Bruijn index of `variable`, or `None` when it is not
    /// bound.
    ///
    /// When the variable is bound multiple times (shadowing), the innermost
    /// binder wins, which is the usual lexical scoping.
    pub fn index_of(&self, variable: &V) -> Option<usize> {
        self.bound.iter().rev().position(|bound| bound == variable)
    }

    /// Returns the number of variables currently in scope.
    pub fn len(&self) -> usize {
        self.bound.len()
    }

    /// Returns true iff no variable is bound.
    pub fn is_empty(&self) -> bool {
        self.bound.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use merc_utilities::test_logger;

    use super::BindingScope;

    fn variable(name: &'static str) -> &'static str {
        name
    }

    #[test]
    fn test_innermost_variable_has_index_zero() {
        test_logger();
        let (x, y) = (variable("x"), variable("y"));

        let mut scope = BindingScope::new();
        assert_eq!(scope.push([x]), 1);
        assert_eq!(scope.push([y]), 1);

        assert_eq!(scope.index_of(&y), Some(0));
        assert_eq!(scope.index_of(&x), Some(1));
        assert_eq!(scope.index_of(&variable("z")), None);
    }

    #[test]
    fn test_multi_variable_binder_numbers_in_list_order() {
        test_logger();
        let (x, y) = (variable("x"), variable("y"));

        let mut nested = BindingScope::new();
        nested.push([x]);
        nested.push([y]);

        let mut flat = BindingScope::new();
        assert_eq!(flat.push([x, y]), 2);

        assert_eq!(flat.index_of(&x), nested.index_of(&x));
        assert_eq!(flat.index_of(&y), nested.index_of(&y));
    }

    #[test]
    fn test_pop_restores_outer_scope() {
        test_logger();
        let (x, y) = (variable("x"), variable("y"));

        let mut scope = BindingScope::new();
        scope.push([x]);
        let pushed = scope.push([y]);
        scope.pop(pushed);

        assert_eq!(scope.len(), 1);
        assert_eq!(scope.index_of(&x), Some(0));
        assert_eq!(scope.index_of(&y), None);
    }

    #[test]
    fn test_shadowing_resolves_to_innermost_binder() {
        test_logger();
        let x = variable("x");

        let mut scope = BindingScope::new();
        scope.push([x]);
        scope.push([variable("y")]);
        scope.push([x]);

        assert_eq!(scope.index_of(&x), Some(0), "the innermost x shadows the outer one");
    }
}
