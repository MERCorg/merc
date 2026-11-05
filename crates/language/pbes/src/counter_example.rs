/// Returns true iff `name` is a counter-example propositional variable name.
pub fn is_counter_example_name(name: &str) -> bool {
    counter_example_prefix(name).is_some()
}

/// Returns true iff `name` is a *positive* counter-example propositional variable name (`Zpos_`).
pub fn is_counter_example_positive(name: &str) -> bool {
    counter_example_prefix(name) == Some("Zpos_")
}

/// Returns the LPS summand index `<n>` embedded in a counter-example name `Z{pos,neg}_<n>_...`.
pub fn counter_example_index(name: &str) -> Option<usize> {
    let prefix = counter_example_prefix(name)?;
    let rest = &name[prefix.len()..];
    rest[..rest.find('_')?].parse().ok()
}

/// Returns the PVI's counter example prefix if `name` matches the
/// counter-example naming convention `Z{pos,neg}_<index>_...`.
fn counter_example_prefix(name: &str) -> Option<&'static str> {
    for prefix in ["Zpos_", "Zneg_"] {
        if let Some(rest) = name.strip_prefix(prefix)
            && let Some(index_len) = rest.find('_')
            && index_len > 0
            && rest[..index_len].bytes().all(|byte| byte.is_ascii_digit())
        {
            return Some(prefix);
        }
    }
    None
}
