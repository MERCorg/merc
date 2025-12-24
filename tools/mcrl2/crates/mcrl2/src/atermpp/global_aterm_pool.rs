use std::fmt::Debug;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::LazyLock;

use log::info;
use log::trace;
use mcrl2_sys::atermpp::ffi::mcrl2_aterm_mark_address;
use mcrl2_sys::atermpp::ffi::mcrl2_aterm_pool_capacity;
use mcrl2_sys::atermpp::ffi::mcrl2_aterm_pool_enable_automatic_garbage_collection;
use mcrl2_sys::atermpp::ffi::mcrl2_aterm_pool_size;
use parking_lot::Mutex;

use mcrl2_sys::atermpp::ffi;
use merc_collections::ProtectionSet;

use crate::atermpp::BfTermPool;

use super::Markable;

/// This newtype is necessary since plain pointers cannot be marked as Send.
/// However since terms are immutable pointers it is fine to read them in multiple
/// threads.
#[derive(Clone, Debug)]
pub(crate) struct ATermPtr {
    pub(crate) ptr: *const ffi::_aterm,
}

impl ATermPtr {
    pub(crate) fn new(ptr: *const ffi::_aterm) -> ATermPtr {
        ATermPtr { ptr }
    }
}

unsafe impl Send for ATermPtr {}

/// The protection set for terms.
pub(crate) type SharedProtectionSet = Arc<BfTermPool<ProtectionSet<ATermPtr>>>;

/// The protection set for containers.
pub(crate) type SharedContainerProtectionSet = Arc<BfTermPool<ProtectionSet<Arc<dyn Markable + Sync + Send>>>>;

/// The single global (singleton) term pool.
pub(crate) struct GlobalTermPool {
    /// The protection sets for thread local terms.
    thread_protection_sets: Vec<Option<SharedProtectionSet>>,
    thread_container_sets: Vec<Option<SharedContainerProtectionSet>>,
}

impl GlobalTermPool {
    fn new() -> GlobalTermPool {
        // For the protection sets we disable automatic garbage collection, and call it when it is allowed.
        mcrl2_aterm_pool_enable_automatic_garbage_collection(false);

        GlobalTermPool {
            thread_protection_sets: vec![],
            thread_container_sets: vec![],
        }
    }

    /// Register a new thread term pool to manage thread specific aspects.
    pub(crate) fn register_thread_term_pool(&mut self) -> (SharedProtectionSet, SharedContainerProtectionSet, usize) {
        trace!("Registered ThreadTermPool {}", self.thread_protection_sets.len());

        // Register a protection set into the global set.
        // TODO: use existing free spots.
        let protection_set = Arc::new(BfTermPool::new(ProtectionSet::new()));
        self.thread_protection_sets.push(Some(protection_set.clone()));

        let container_protection_set = Arc::new(BfTermPool::new(ProtectionSet::new()));
        self.thread_container_sets.push(Some(container_protection_set.clone()));

        (
            protection_set,
            container_protection_set,
            self.thread_container_sets.len() - 1,
        )
    }

    /// Drops the thread term pool with the given index.
    pub(crate) fn drop_thread_term_pool(&mut self, index: usize) {
        self.thread_protection_sets[index] = None;
        self.thread_container_sets[index] = None;
        trace!("Removed ThreadTermPool {}", index);
    }

    /// Marks the terms in all protection sets.
    fn mark_protection_sets(&mut self, mut todo: Pin<&mut ffi::term_mark_stack>) {
        trace!("Marking terms:");
        for set in self.thread_protection_sets.iter().flatten() {
            // Do not lock since we acquired a global lock.
            unsafe {
                let protection_set = set.get();

                for (root, term) in protection_set.iter() {
                    mcrl2_aterm_mark_address(term.ptr.as_ref().expect("Must be a valid pointer"), todo.as_mut());

                    trace!("Marked {:?}, index {root}", term.ptr);
                }
            }
        }

        for set in self.thread_container_sets.iter().flatten() {
            // Do not lock since we acquired a global lock.
            unsafe {
                let protection_set = set.get();

                for (root, container) in protection_set.iter() {
                    container.mark(todo.as_mut());

                    let length = container.len();

                    trace!("Marked container index {root}, size {}", length);
                }
            }
        }

        info!("Collecting garbage \n{:?}", self);
    }

    /// Counts the number of terms in all protection sets.
    fn protection_set_size(&self) -> usize {
        let mut result = 0;
        for set in self.thread_protection_sets.iter().flatten() {
            result += set.read().len();
        }

        // Gather the sizes of all containers
        for set in self.thread_container_sets.iter().flatten() {
            for (_index, container) in set.read().iter() {
                result += container.len();
            }
        }
        result
    }

    /// Returns the number of terms in the pool.
    pub fn len(&self) -> usize {
        mcrl2_aterm_pool_size()
    }

    /// Returns the number of terms in the pool.
    pub fn capacity(&self) -> usize {
        mcrl2_aterm_pool_capacity()
    }
}

impl Debug for GlobalTermPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut protected = 0;
        let mut total = 0;
        let mut max = 0;

        for set in self.thread_protection_sets.iter().flatten() {
            let protection_set = set.read();
            protected += protection_set.len();
            total += protection_set.number_of_insertions();
            max += protection_set.maximum_size();
        }

        let mut num_containers = 0;
        let mut max_containers = 0;
        let mut total_containers = 0;
        let mut inside_containers = 0;
        for set in self.thread_container_sets.iter().flatten() {
            let protection_set = set.read();
            num_containers += protection_set.len();
            total_containers += protection_set.number_of_insertions();
            max_containers += protection_set.maximum_size();

            for (_index, container) in protection_set.iter() {
                inside_containers += container.len();
            }
        }

        write!(
            f,
            "{} terms, max capacity {}, {} variables in thread root sets and {} in {} containers (term set {} insertions, max {}; container set {} insertions, max {})",
            self.len(),
            self.capacity(),
            protected,
            inside_containers,
            num_containers,
            total,
            max,
            total_containers,
            max_containers,
        )
    }
}

/// This is the global set of protection sets that are managed by the ThreadTermPool
pub(crate) static GLOBAL_TERM_POOL: LazyLock<Mutex<GlobalTermPool>> =
    LazyLock::new(|| Mutex::new(GlobalTermPool::new()));

/// Marks the terms in all protection sets using the global aterm pool.
pub(crate) fn mark_protection_sets(todo: Pin<&mut ffi::term_mark_stack>) {
    GLOBAL_TERM_POOL.lock().mark_protection_sets(todo);
}

/// Counts the number of terms in all protection sets.
pub(crate) fn protection_set_size() -> usize {
    GLOBAL_TERM_POOL.lock().protection_set_size()
}
