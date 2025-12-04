
// Helper trait to concatenate permutations
pub trait CloneIterator: Iterator {
    fn clone_boxed(&self) -> Box<dyn CloneIterator<Item = Self::Item>>;
}

impl<T, I> CloneIterator for I
where
    I: Iterator<Item = T> + Clone + 'static,
{
    fn clone_boxed(&self) -> Box<dyn CloneIterator<Item = Self::Item>> {
        Box::new(self.clone())
    }
}

impl<T: Clone + 'static> Clone for Box<dyn CloneIterator<Item = T>> {
    fn clone(&self) -> Self {
        self.clone_boxed()
    }
}
