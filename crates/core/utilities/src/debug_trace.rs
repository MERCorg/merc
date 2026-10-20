/// Prints debug trace information via `log::trace!` when the `merc_debug-trace` feature is
/// enabled; otherwise a no-op.
///
/// # Examples
///
/// ```
/// # use merc_utilities::debug_trace;
/// debug_trace!("Processing item {}", 42);
/// debug_trace!("Complex calculation result: {:#?}", [1, 2, 3]);
/// ```
#[macro_export]
#[cfg(feature = "merc_debug-trace")]
macro_rules! debug_trace {
    ($($arg:tt)*) => {
        {
            log::trace!($($arg)*);
        }
    };
}

#[macro_export]
#[cfg(not(feature = "merc_debug-trace"))]
macro_rules! debug_trace {
    ($($arg:tt)*) => {{
        // No-op when merc_debug-trace is not enabled
    }};
}

/// Expands to a `use` of the given item when the `merc_debug-trace` feature is enabled;
/// otherwise a no-op, so the import doesn't trigger an unused-import warning when disabled.
///
/// # Examples
///
/// ```
/// # use merc_utilities::debug_use;
/// debug_use!(std::collections::HashMap);
/// debug_use!(std::fmt::Debug);
/// ```
#[macro_export]
#[cfg(feature = "merc_debug-trace")]
macro_rules! debug_use {
    ($($item:tt)*) => {
        use $($item)*;
    };
}

#[macro_export]
#[cfg(not(feature = "merc_debug-trace"))]
macro_rules! debug_use {
    ($($item:tt)*) => {
        // No-op when merc_debug-trace is not enabled
    };
}
