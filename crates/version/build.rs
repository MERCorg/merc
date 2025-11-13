use std::process::Command;

fn main() {
    if let Ok(output) = Command::new("git").args(["rev-parse", "HEAD"]).output() {
        let output = String::from_utf8(output.stdout).expect("Invalid UTF-8 in git output");
        let build_hash = output.trim();
        println!("cargo:rustc-env=BUILD_HASH={build_hash}");
    } else {
        println!("cargo:rustc-env=BUILD_HASH=UNKNOWN");
    }
}
