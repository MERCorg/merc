//! FFI boundary between the host (which owns the global term pool) and the
//! dynamically compiled-and-loaded rewriter library.
//!
//! # Why a vtable
//!
//! The generated rewriter is compiled as a separate `cdylib` and loaded with
//! `dlopen`. It therefore has its own statically-linked copy of `merc_aterm`
//! and, crucially, its own copy of every global (the term pool, the `dashmap`
//! shard tables, ...). Sharing a `#[repr(Rust)]` structure such as the term
//! pool across that boundary by raw pointer is unsound: Rust gives no layout
//! guarantee for `repr(Rust)` types across independent compilations, so the two
//! sides can disagree on where a field lives.
//!
//! Instead, *all* term-pool access happens on the host side. The host fills in
//! a [`SabreRewriteVTable`] of `extern "C-unwind"` function pointers and passes
//! it to the loaded library through [`set_rewrite_vtable`]. The generated code
//! only ever shuffles around opaque, `#[repr(C)]`, pointer-sized handles and
//! dispatches every real operation back into the host through the vtable. Since
//! only a `#[repr(C)]` vtable and `#[repr(C)]` handles cross the boundary, this
//! works regardless of the `rustc` version used to build either side.

use std::ffi::c_void;
use std::marker::PhantomData;
use std::sync::OnceLock;

mod host;

pub use host::*;

/// Table of host-provided operations. Every field is an `extern "C-unwind"`
/// function pointer into the host binary, so the ABI is stable across `rustc`
/// versions. The generated library never implements these; it only calls them.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SabreRewriteVTable {
    /// Returns the number of data arguments of the term.
    pub arity: unsafe extern "C-unwind" fn(node: *const c_void) -> usize,
    /// Returns the `index`-th data argument of the term.
    pub data_arg: unsafe extern "C-unwind" fn(node: *const c_void, index: usize) -> *const c_void,
    /// Returns the head data function symbol of the term.
    pub data_function_symbol: unsafe extern "C-unwind" fn(node: *const c_void) -> *const c_void,
    /// Returns the operation identifier of a data function symbol.
    pub operation_id: unsafe extern "C-unwind" fn(node: *const c_void) -> usize,
    /// Constructs (and protects) a data expression from a head symbol and `len`
    /// argument references. With `len == 0` the result is the constant denoted
    /// by the head symbol.
    pub create: unsafe extern "C-unwind" fn(
        head: *const c_void,
        args: *const DataExpressionRefFFI<'static>,
        len: usize,
    ) -> DataExpressionFFI,
    /// Protects an existing term node, returning an owned handle.
    pub protect: unsafe extern "C-unwind" fn(node: *const c_void) -> DataExpressionFFI,
    /// Releases an owned handle previously returned by `create`/`protect`.
    pub destroy: unsafe extern "C-unwind" fn(handle: *mut c_void),
}

/// Storage for the vtable installed by the host. Set once, at library load
/// time, through [`set_rewrite_vtable`].
static VTABLE: OnceLock<SabreRewriteVTable> = OnceLock::new();

/// Installs the host-provided [`SabreRewriteVTable`]. Called once by the loaded
/// library's `initialise` entry point.
///
/// # Safety
///
/// `vtable` must point to a valid [`SabreRewriteVTable`] whose function pointers
/// remain valid for the lifetime of the process (they point into the host).
pub unsafe fn set_rewrite_vtable(vtable: *const SabreRewriteVTable) {
    // The vtable is copied by value, so the caller's storage need not outlive
    // this call. A second call is ignored: the host installs exactly one.
    let _ = VTABLE.set(unsafe { *vtable });
}

/// Returns the installed vtable, panicking if the library was not initialised.
#[inline]
fn vtable() -> &'static SabreRewriteVTable {
    VTABLE
        .get()
        .expect("the rewrite vtable was not installed; call `initialise` first")
}

/// An owned, protected data expression. Opaque on both sides of the boundary:
/// `node` is the address of the shared term node (used for cheap, pure
/// comparisons and to derive borrows), and `handle` owns the host-side
/// protection that keeps the node alive.
#[repr(C)]
pub struct DataExpressionFFI {
    node: *const c_void,
    handle: *mut c_void,
}

impl DataExpressionFFI {
    /// Constructs a data expression from a head symbol and its arguments.
    pub fn create(symbol: DataExpressionRefFFI<'_>, args: &[DataExpressionRefFFI<'_>]) -> DataExpressionFFI {
        // SAFETY: the vtable is installed and `args` is a valid slice. The
        // lifetime on the references is erased for the call; the host only
        // reads them synchronously.
        unsafe { (vtable().create)(symbol.node, args.as_ptr().cast(), args.len()) }
    }

    /// Constructs the constant denoted by the given data function symbol.
    pub fn constant(symbol: DataExpressionRefFFI<'_>) -> DataExpressionFFI {
        Self::create(symbol, &[])
    }

    /// Returns a borrowed reference to this expression.
    pub fn copy(&self) -> DataExpressionRefFFI<'_> {
        DataExpressionRefFFI::from_raw(self.node)
    }

    /// Returns a fresh owned, protected copy of this expression.
    pub fn protect(&self) -> DataExpressionFFI {
        unsafe { (vtable().protect)(self.node) }
    }

    /// Returns the shared term node address, used for maximal-sharing equality.
    pub fn shared(&self) -> *const c_void {
        self.node
    }
}

impl Drop for DataExpressionFFI {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { (vtable().destroy)(self.handle) }
        }
    }
}

/// A borrowed reference to a data expression: just the address of the shared
/// term node. Valid for as long as something keeps that node protected.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DataExpressionRefFFI<'a> {
    node: *const c_void,
    _marker: PhantomData<&'a ()>,
}

impl<'a> DataExpressionRefFFI<'a> {
    /// Builds a reference from a raw shared term node address.
    ///
    /// Generated code uses this to refer to symbols and terms whose addresses
    /// were baked in at code-generation time.
    ///
    /// # Safety
    ///
    /// `node` must be the address of a live shared term node that stays
    /// protected from garbage collection for the lifetime `'a`.
    pub unsafe fn from_ptr(node: usize) -> Self {
        Self::from_raw(node as *const c_void)
    }

    /// Builds a reference from a raw shared term node pointer.
    pub(crate) fn from_raw(node: *const c_void) -> Self {
        Self {
            node,
            _marker: PhantomData,
        }
    }

    /// Returns the shared term node address, used for maximal-sharing equality.
    pub fn shared(&self) -> *const c_void {
        self.node
    }

    /// Returns a copy of this reference.
    pub fn copy(&self) -> DataExpressionRefFFI<'a> {
        *self
    }

    /// Returns the number of data arguments of this expression.
    pub fn arity(&self) -> usize {
        unsafe { (vtable().arity)(self.node) }
    }

    /// Returns the `index`-th data argument of this expression.
    pub fn data_arg(&self, index: usize) -> DataExpressionRefFFI<'a> {
        DataExpressionRefFFI::from_raw(unsafe { (vtable().data_arg)(self.node, index) })
    }

    /// Returns the head data function symbol of this expression.
    pub fn data_function_symbol(&self) -> DataFunctionSymbolRefFFI<'a> {
        DataFunctionSymbolRefFFI::from_raw(unsafe { (vtable().data_function_symbol)(self.node) })
    }

    /// Protects this expression, returning an owned handle.
    pub fn protect(&self) -> DataExpressionFFI {
        unsafe { (vtable().protect)(self.node) }
    }
}

/// A borrowed reference to a data function symbol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DataFunctionSymbolRefFFI<'a> {
    node: *const c_void,
    _marker: PhantomData<&'a ()>,
}

impl<'a> DataFunctionSymbolRefFFI<'a> {
    /// Builds a reference from a raw shared term node pointer.
    pub(crate) fn from_raw(node: *const c_void) -> Self {
        Self {
            node,
            _marker: PhantomData,
        }
    }

    /// Returns the operation identifier of this data function symbol.
    pub fn operation_id(&self) -> usize {
        unsafe { (vtable().operation_id)(self.node) }
    }
}

impl<'a> From<DataFunctionSymbolRefFFI<'a>> for DataExpressionRefFFI<'a> {
    fn from(symbol: DataFunctionSymbolRefFFI<'a>) -> Self {
        // A data function symbol is itself a data expression.
        DataExpressionRefFFI::from_raw(symbol.node)
    }
}
