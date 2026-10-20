use std::alloc::Layout;
use std::alloc::LayoutError;
use std::fmt;
use std::hash::Hash;
use std::mem::offset_of;
use std::ptr::NonNull;
use std::ptr::slice_from_raw_parts_mut;

use equivalent::Equivalent;
use merc_unsafety::Erasable;
use merc_unsafety::ErasedPtr;
use merc_unsafety::SliceDst;
use merc_unsafety::StablePointer;
use merc_unsafety::repr_c;
use merc_utilities::AtomicTaggedPtr;

use crate::ATermRef;
use crate::Symb;
use crate::SymbolRef;
use crate::Term;
use crate::Transmutable;
use crate::storage::SharedSymbol;

/// The underlying type of terms that are maximally shared.
///
/// # Details
///
/// Uses a C representation and is a dynamically sized type for compact memory
/// usage, implementing [SliceDst] and [Erasable]. This allows us to avoid
/// storing the length and capacity of an underlying vector. As such this is
/// even more compact than `smallvec`. Arguments are stored as [ATermRef] slices.
#[repr(C)]
pub struct SharedTerm {
    symbol: TermSymbol,
    arguments: [ATermRef<'static>],
}

/// The function symbol of a stored term, whose lowest address bit marks the term as reachable
/// during garbage collection.
///
/// The collector sets and clears the mark while it holds the term pool exclusively, but other
/// threads may read the symbol meanwhile, which is why it is atomic and reads ignore the mark.
#[repr(C)]
pub(crate) struct TermSymbol {
    ptr: AtomicTaggedPtr<SharedSymbol>,

    /// Holds the symbol's debug reference counter, see [`StablePointer`].
    #[cfg(debug_assertions)]
    reference: SymbolRef<'static>,
}

impl TermSymbol {
    /// Stores the given symbol, which the term keeps alive because marking a term marks its symbol.
    pub(crate) fn new(symbol: &SymbolRef<'_>) -> Self {
        Self {
            // SAFETY: a symbol is a sized element of the symbol pool, so `ptr` does not read it.
            ptr: AtomicTaggedPtr::new(unsafe { symbol.shared().ptr() }),
            // SAFETY: the symbol lives as long as the term, see above.
            #[cfg(debug_assertions)]
            reference: unsafe { SymbolRef::from_index(symbol.shared()) },
        }
    }

    /// Returns the symbol.
    pub(crate) fn get(&self) -> SymbolRef<'_> {
        // SAFETY: the stored pointer was not null, and masking the mark keeps it so.
        let ptr = unsafe { NonNull::new_unchecked(self.ptr.load()) };

        // SAFETY: `ptr` points to the symbol that this term keeps alive, see `new`, which is also
        // the symbol of `reference`.
        #[cfg(debug_assertions)]
        let index = unsafe { StablePointer::from_related_ptr(ptr, self.reference.shared()) };
        #[cfg(not(debug_assertions))]
        let index = unsafe { StablePointer::from_ptr(ptr) };

        // SAFETY: the returned reference borrows `self`, which keeps the symbol alive.
        unsafe { SymbolRef::from_index(&index) }
    }

    /// Marks the term and returns true iff it was not marked before.
    pub(crate) fn mark(&self) -> bool {
        !self.ptr.set_tag()
    }

    /// Returns true iff the term is marked.
    pub(crate) fn is_marked(&self) -> bool {
        self.ptr.is_tagged()
    }

    /// Removes the mark of the term.
    pub(crate) fn unmark(&self) {
        self.ptr.clear_tag()
    }
}

impl PartialEq for TermSymbol {
    fn eq(&self, other: &Self) -> bool {
        self.ptr.load() == other.ptr.load()
    }
}

impl Eq for TermSymbol {}

impl Hash for TermSymbol {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Must equal the hash of the symbol itself, which lookups use.
        self.get().hash(state);
    }
}

impl fmt::Debug for TermSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.get())
    }
}

impl PartialEq for SharedTerm {
    fn eq(&self, other: &Self) -> bool {
        self.symbol == other.symbol && self.arguments == other.arguments
    }
}

impl Eq for SharedTerm {}

/// Note that the length is stored in the symbol's arity
unsafe impl SliceDst for SharedTerm {
    fn layout_for(len: usize) -> Result<Layout, LayoutError> {
        let header_layout = Layout::new::<TermSymbol>();
        let slice_layout = Layout::array::<ATermRef<'static>>(len)?;

        repr_c(&[header_layout, slice_layout])
    }

    fn retype(ptr: std::ptr::NonNull<[()]>) -> NonNull<Self> {
        unsafe { NonNull::new_unchecked(ptr.as_ptr() as *mut _) }
    }

    fn length(&self) -> usize {
        self.symbol().arity()
    }
}

unsafe impl Erasable for SharedTerm {
    fn erase(this: NonNull<Self>) -> ErasedPtr {
        this.cast()
    }

    unsafe fn unerase(this: ErasedPtr) -> NonNull<Self> {
        unsafe {
            // Every term starts with its symbol, whose arity is the number of arguments.
            let len = (*this.as_ptr().cast::<TermSymbol>()).get().arity();

            let raw = NonNull::new_unchecked(slice_from_raw_parts_mut(this.as_ptr().cast(), len));
            Self::retype(raw)
        }
    }
}

impl fmt::Debug for SharedTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SharedTerm {{ symbol: {:?}, arguments: {:?} }}",
            self.symbol,
            self.arguments()
        )
    }
}

impl SharedTerm {
    /// Returns the symbol of the term.
    pub fn symbol(&self) -> SymbolRef<'_> {
        self.symbol.get()
    }

    /// Marks the term as reachable and returns true iff it was not marked before.
    pub(crate) fn mark(&self) -> bool {
        self.symbol.mark()
    }

    /// Returns true iff the term is marked as reachable.
    pub(crate) fn is_marked(&self) -> bool {
        self.symbol.is_marked()
    }

    /// Removes the reachability mark of the term.
    pub(crate) fn unmark(&self) {
        self.symbol.unmark()
    }

    /// Returns the arguments of the term.
    pub fn arguments(&self) -> &[ATermRef<'_>] {
        // SAFETY: The returned lifetime is bound to the borrow of `self` by the signature.
        unsafe { self.arguments.transmute_lifetime() }
    }

    /// Returns a unique index for this shared term.
    pub fn index(&self) -> usize {
        self as *const Self as *const u8 as usize
    }

    /// Returns the length for a [SharedTermLookup]
    pub(crate) fn length_for(object: &SharedTermLookup) -> usize {
        object.arguments.len()
    }

    /// Constructs an uninitialised ptr from a [SharedTermLookup]
    pub(crate) unsafe fn construct(ptr: *mut SharedTerm, object: &SharedTermLookup) {
        let header_layout = Layout::new::<TermSymbol>();
        let slice_layout =
            Layout::array::<ATermRef<'static>>(object.arguments.len()).expect("Layout should not exceed isize");

        let (_, slice_offset) = header_layout
            .extend(slice_layout)
            .expect("Layout should not exceed isize");
        unsafe {
            ptr.cast::<TermSymbol>().write(TermSymbol::new(&object.symbol));

            for (index, argument) in object.arguments.iter().enumerate() {
                ptr.byte_offset(slice_offset as isize)
                    .cast::<ATermRef<'static>>()
                    .add(index)
                    .write(ATermRef::from_index(argument.shared()));
            }
        }
    }
}

impl Hash for SharedTerm {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.symbol.hash(state);
        self.arguments.hash(state);
    }
}

/// A cheap reference to the elements of a [SharedTerm] that can be used for
/// lookup of terms without allocating.
pub(crate) struct SharedTermLookup<'a> {
    pub(crate) symbol: SymbolRef<'a>,
    pub(crate) arguments: &'a [ATermRef<'a>],
}

impl Equivalent<SharedTerm> for SharedTermLookup<'_> {
    fn equivalent(&self, other: &SharedTerm) -> bool {
        self.symbol == other.symbol() && self.arguments == &other.arguments
    }
}

/// This Hash implement must be the same as for [SharedTerm]
impl Hash for SharedTermLookup<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.symbol.hash(state);
        self.arguments.hash(state);
    }
}

// `symbol` must be at offset 0 in all term representations so that any pointer to a term.
const _: () = assert!(offset_of!(SharedTerm, symbol) == 0);

#[cfg(test)]
mod tests {
    use allocator_api2::alloc::Global;

    use merc_unsafety::AllocatorDst;
    #[cfg(not(debug_assertions))]
    use merc_unsafety::SliceDst;

    use crate::ATerm;
    use crate::Symbol;
    use crate::Term;
    use crate::storage::SharedTerm;
    use crate::storage::SharedTermLookup;

    #[test]
    #[cfg(not(debug_assertions))]
    fn test_shared_symbol_size() {
        // Cannot be a const assertion since the size depends on the length.
        assert_eq!(
            SharedTerm::layout_for(0)
                .expect("The layout should not overflow")
                .size(),
            1 * std::mem::size_of::<usize>(),
            "A SharedTerm without arguments should be the same size as the Symbol"
        );

        // A thin `ATermRef` is a single word, so the arguments no longer carry redundant slice
        // length metadata: a binary term is the symbol word plus two argument words.
        assert_eq!(
            SharedTerm::layout_for(2)
                .expect("The layout should not overflow")
                .size(),
            3 * std::mem::size_of::<usize>(),
            "A SharedTerm with arity two should be the same size as the Symbol and two ATermRef arguments"
        );
    }

    #[test]
    fn test_shared_term_lookup() {
        let symbol = Symbol::new("a", 2);

        let term = ATerm::constant(&Symbol::new("b", 0));

        let lookup = SharedTermLookup {
            symbol: symbol.copy(),
            arguments: &[term.copy(), term.copy()],
        };

        let ptr = Global.allocate_slice_dst(2).expect("Could not allocate slice dst");

        unsafe {
            SharedTerm::construct(ptr.as_ptr(), &lookup);
            assert_eq!(
                ptr.as_ref().symbol(),
                symbol.copy(),
                "The symbol should match the lookup symbol"
            );
            assert_eq!(
                ptr.as_ref().arguments()[0],
                term.copy(),
                "The arguments should match the lookup arguments"
            );
            assert_eq!(
                ptr.as_ref().arguments()[1],
                term.copy(),
                "The arguments should match the lookup arguments"
            );
        }

        Global.deallocate_slice_dst(ptr, 2);
    }
}
