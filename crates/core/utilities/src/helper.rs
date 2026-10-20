use std::cell::Cell;
use std::marker::PhantomData;
use std::sync::MutexGuard;

/// Indicates that a type is !Sync
pub type PhantomUnsync = PhantomData<Cell<()>>;

/// Indicates that a type is !Send
pub type PhantomUnsend = PhantomData<MutexGuard<'static, ()>>;
