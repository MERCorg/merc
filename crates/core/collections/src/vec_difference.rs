use std::cmp::Ordering;
use std::marker::PhantomData;

/// A lazy iterator that yields the difference of two sorted sequences. The elements are yielded in sorted order.
pub(crate) struct Difference<'a, T, I> {
    pub(crate) self_iter: I,
    pub(crate) other_iter: I,
    pub(crate) other_next: Option<&'a T>,
    pub(crate) marker: PhantomData<&'a T>,
}

impl<'a, T, I> Iterator for Difference<'a, T, I>
where
    I: Iterator<Item = &'a T>,
    T: Ord + PartialEq,
{
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.other_next.is_none() {
            self.other_next = self.other_iter.next();
        }

        for self_val in self.self_iter.by_ref() {
            loop {
                match self.other_next {
                    Some(other_val) => {
                        match self_val.cmp(other_val) {
                            Ordering::Equal => {
                                self.other_next = self.other_iter.next();
                                break;
                            }
                            Ordering::Greater => self.other_next = self.other_iter.next(),
                            Ordering::Less => return Some(self_val), // self_val is smaller, yield it
                        }
                    }
                    None => return Some(self_val), // other is exhausted, yield remaining elements of self
                }
            }
        }

        None
    }
}
