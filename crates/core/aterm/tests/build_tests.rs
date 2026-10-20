#[test]
#[cfg_attr(miri, ignore)]
fn test_soundness() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/input/aterm_lifetime.rs");
    t.compile_fail("tests/input/aterm_container_lifetime.rs");
    t.compile_fail("tests/input/derived_term_lifetime.rs");
    t.compile_fail("tests/input/protected_write_lifetime.rs");
}
