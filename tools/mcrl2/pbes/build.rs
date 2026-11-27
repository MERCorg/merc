fn main() {
    // The compiling rewriter (jittyc) loads a shared library that resolves the
    // mCRL2 symbols from this executable. They are linked statically, so they
    // must be exported in the dynamic symbol table. The `-rdynamic` flag of the
    // `mcrl2` build script only applies to that crate's own targets.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo::rustc-link-arg-bins=-rdynamic");
    }
}
