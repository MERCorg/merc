use std::process::Command;

fn main() {
    // NOTE: we deliberately emit no `cargo:rerun-if-changed` for `.git/HEAD`, so
    // the baked hash can go stale across commits in incremental dev builds. That
    // is acceptable because this hash is only meaningful for packaged releases,
    // which are built from a clean checkout where the build script always runs.
    if let Ok(output) = Command::new("git").args(["rev-parse", "HEAD"]).output() {
        let output = String::from_utf8(output.stdout).expect("Invalid UTF-8 in git output");
        let build_hash = output.trim();
        println!("cargo:rustc-env=BUILD_HASH={build_hash}");
    } else {
        println!("cargo:rustc-env=BUILD_HASH=UNKNOWN");
    }
}
