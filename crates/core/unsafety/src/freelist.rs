use std::cell::Cell;
use std::marker::PhantomData;
use std::ptr::NonNull;

/// Intrusive node requirements for use in [`FreeList`].
///
/// # Safety
///
/// Implementors must guarantee that `get_next` and `set_next` read and write
/// the same link field, and that the field is valid for all nodes managed by
/// a corresponding freelist.
pub unsafe trait FreeListEntry: Sized {
    /// Returns the next pointer for `ptr` (or null).
    ///
    /// # Safety
    ///
    /// `ptr` must be a valid pointer to a node managed by the corresponding
    /// freelist.
    unsafe fn get_next(ptr: *mut Self) -> *mut Self;

    /// Sets the next pointer for `ptr`.
    ///
    /// # Safety
    ///
    /// `ptr` must be a valid pointer to a node managed by the corresponding
    /// freelist, and `next` must be either null or a valid pointer to a node
    /// managed by the same freelist.
    unsafe fn set_next(ptr: *mut Self, next: *mut Self);
}

/// Intrusive freelist based on a stack.
pub struct FreeList<T: FreeListEntry> {
    /// The head of the freelist. Null means empty.
    head: Cell<*mut T>,

    /// We implement Send and Sync manually.
    _marker: PhantomData<*mut T>,
}

// Safety: Transferring a FreeList to another thread is safe if T is Send.
unsafe impl<T: FreeListEntry + Send> Send for FreeList<T> {}

impl<T: FreeListEntry> Default for FreeList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: FreeListEntry> FreeList<T> {
    pub fn new() -> Self {
        Self {
            head: Cell::new(std::ptr::null_mut()),
            _marker: PhantomData,
        }
    }

    /// Pops one entry from the freelist.
    ///
    /// Relies on the freelist invariant that every node on the list is valid, which is
    /// guaranteed by the contracts of [`FreeList::push`] and [`FreeList::set_head`].
    #[cfg_attr(
        kani,
        kani::requires(self.head.get().is_null() || kani::mem::can_dereference(self.head.get()))
    )]
    #[cfg_attr(kani, kani::modifies(&self.head))]
    pub fn try_pop(&self) -> Option<NonNull<T>> {
        let node = NonNull::new(self.head.get())?;

        // Safety: `head` is non-null, and nodes on the list are valid per the push/set_head contracts.
        let next = unsafe { T::get_next(node.as_ptr()) };
        self.head.set(next);

        Some(node)
    }

    /// Pushes an entry onto the freelist.
    ///
    /// # Safety
    ///
    /// `entry` must point to a valid node whose link field may be written, and the node must
    /// remain valid until it is popped from the list (the list takes ownership of it).
    #[cfg_attr(kani, kani::requires(kani::mem::can_dereference(entry.as_ptr())))]
    #[cfg_attr(kani, kani::modifies(&self.head, entry.as_ptr()))]
    pub unsafe fn push(&self, entry: NonNull<T>) {
        let ptr = entry.as_ptr();
        let head = self.head.get();

        // Safety: caller transfers ownership of a valid node; writing its next link is valid.
        unsafe { T::set_next(ptr, head) };
        self.head.set(ptr);
    }

    /// Returns an iterator over freelist entries.
    ///
    /// # Safety
    ///
    /// The list must not be mutated while the iterator is live, and every node
    /// reachable from the head must remain valid for the duration of the walk.
    pub unsafe fn iter(&self) -> FreeListIterator<T> {
        FreeListIterator {
            current: NonNull::new(self.head.get()),
        }
    }

    /// Clears the freelist head.
    pub fn clear(&mut self) {
        self.head.set(std::ptr::null_mut());
    }

    /// Returns whether the freelist is currently empty.
    pub fn is_empty(&self) -> bool {
        self.head.get().is_null()
    }

    /// Replaces the freelist head with `head`.
    ///
    /// # Safety
    ///
    /// `head` must be either null or a valid linked list of nodes
    /// managed by this freelist.
    pub unsafe fn set_head(&self, head: *mut T) {
        self.head.set(head);
    }
}

/// Iterator over entries in a [`FreeList`].
pub struct FreeListIterator<T: FreeListEntry> {
    current: Option<NonNull<T>>,
}

impl<T: FreeListEntry> Iterator for FreeListIterator<T> {
    type Item = NonNull<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(current) = self.current {
            // Safety: `current` is a freelist node; its link field is valid to read.
            unsafe {
                self.current = NonNull::new(T::get_next(current.as_ptr()));
            }
            Some(current)
        } else {
            None
        }
    }
}

#[cfg(kani)]
mod verification {
    use super::*;

    /// A minimal `FreeListEntry` node holding only the intrusive link.
    struct Node {
        next: Cell<*mut Node>,
    }

    impl Node {
        fn new() -> Self {
            Self {
                next: Cell::new(std::ptr::null_mut()),
            }
        }
    }

    // Safety: `next` is the single link field; both methods read and write it.
    unsafe impl FreeListEntry for Node {
        unsafe fn get_next(ptr: *mut Self) -> *mut Self {
            // SAFETY: caller guarantees `ptr` points to a valid node.
            unsafe { (*ptr).next.get() }
        }

        unsafe fn set_next(ptr: *mut Self, next: *mut Self) {
            // SAFETY: caller guarantees `ptr` points to a valid node.
            unsafe { (*ptr).next.set(next) };
        }
    }

    /// Checks the `push` contract: given a dereferenceable node pointer, the
    /// call must not violate memory safety (the documented spatial half of
    /// its `# Safety` precondition).
    #[kani::proof_for_contract(FreeList::push)]
    fn freelist_push_contract() {
        let fl: FreeList<Node> = FreeList::new();
        let mut node = Node::new();
        let ptr = NonNull::from(&mut node);

        // SAFETY: `node` is a valid local node that outlives the push call.
        unsafe {
            fl.push(ptr);
        }
    }

    /// Checks the `try_pop` contract with a non-null head.
    ///
    /// `proof_for_contract` only supports a single top-level call to the
    /// contracted function per harness, so the null-head branch gets its own
    /// harness below.
    #[kani::proof_for_contract(FreeList::try_pop)]
    fn freelist_try_pop_contract_non_empty() {
        let fl: FreeList<Node> = FreeList::new();
        let mut node = Node::new();
        let ptr = NonNull::from(&mut node);

        // SAFETY: `node` is a valid local node that outlives the freelist operations.
        unsafe {
            fl.push(ptr);
        }

        fl.try_pop();
    }

    /// Checks the `try_pop` contract with a null head (the empty-list branch
    /// of its precondition).
    #[kani::proof_for_contract(FreeList::try_pop)]
    fn freelist_try_pop_contract_empty() {
        let fl: FreeList<Node> = FreeList::new();
        fl.try_pop();
    }

    #[kani::proof]
    fn freelist_new_is_empty() {
        let fl: FreeList<Node> = FreeList::new();
        assert!(fl.is_empty());
        assert!(fl.try_pop().is_none());
    }

    #[kani::proof]
    fn freelist_push_pop_roundtrip() {
        let fl: FreeList<Node> = FreeList::new();
        let mut node = Node::new();
        let ptr = NonNull::from(&mut node);

        // SAFETY: `node` is a valid local node that outlives the freelist operations.
        unsafe {
            fl.push(ptr);
        }
        assert!(!fl.is_empty());

        let popped = fl.try_pop().expect("freelist contains exactly one entry");
        assert!(std::ptr::eq(popped.as_ptr(), ptr.as_ptr()));
        assert!(fl.is_empty());
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn freelist_push_two_pops_lifo() {
        let fl: FreeList<Node> = FreeList::new();
        let mut a = Node::new();
        let mut b = Node::new();
        let pa = NonNull::from(&mut a);
        let pb = NonNull::from(&mut b);

        // SAFETY: `a` and `b` are valid local nodes that outlive the freelist operations.
        unsafe {
            fl.push(pa);
            fl.push(pb);
        }

        let first = fl.try_pop().expect("non-empty");
        let second = fl.try_pop().expect("non-empty");
        assert!(std::ptr::eq(first.as_ptr(), pb.as_ptr()));
        assert!(std::ptr::eq(second.as_ptr(), pa.as_ptr()));
        assert!(fl.is_empty());
    }

    #[kani::proof]
    fn freelist_clear_makes_empty() {
        let mut fl: FreeList<Node> = FreeList::new();
        let mut node = Node::new();
        // SAFETY: `node` is a valid local node that outlives the freelist operations.
        unsafe {
            fl.push(NonNull::from(&mut node));
        }
        fl.clear();
        assert!(fl.is_empty());
        assert!(fl.try_pop().is_none());
    }
}
