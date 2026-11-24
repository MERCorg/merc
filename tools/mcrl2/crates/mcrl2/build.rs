fn main() {
    // The compiling rewriter loads a shared library that resolves the mCRL2
    // symbols from the executable. They are linked statically, so the tests
    // must export them in the dynamic symbol table.
    if std::env::var_os("CARGO_FEATURE_JITTYC").is_some() && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo::rustc-link-arg=-rdynamic");
    }
}
