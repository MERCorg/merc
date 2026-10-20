/// Extracts the value of the given enum variant, panicking on a variant mismatch.
///
/// Usage: `cast!(instance, Variant)`
#[macro_export]
macro_rules! cast {
    ($target: expr, $pat: path) => {{
        if let $pat(a) = $target {
            a
        } else {
            panic!("mismatch variant when cast to {}", stringify!($pat));
        }
    }};
}
