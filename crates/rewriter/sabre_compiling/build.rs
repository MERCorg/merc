use std::env;
use std::error::Error;
use std::fs;
use std::fs::File;

use std::io::Write;

/// Writes each of `variables` to `writer` as a `NAME = 'value'` line, using an
/// empty value for any variable that is unset.
fn write_env(writer: &mut impl Write, variables: &[&'static str]) -> Result<(), Box<dyn Error>> {
    for var in variables {
        writeln!(writer, "{} = '{}'", var, env::var(var).unwrap_or_default())?;
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = env::var("OUT_DIR")?;
    let mut file = File::create(std::path::Path::new(&out_dir).join("Compilation.toml"))?;

    // Write the development location.
    writeln!(file, "[sabrec]")?;
    writeln!(file, "path = '{}'", fs::canonicalize(".")?.to_string_lossy())?;

    // Record the git commit the host is built from so the runtime-compiled crate
    // can pin its `merc_sabre-ffi` git dependency to the exact same source. This
    // keeps the `#[repr(C)]` vtable layout identical on both sides of the FFI
    // boundary. Best-effort: empty when git is unavailable (e.g. packaged build).
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default();
    writeln!(file, "commit = '{commit}'")?;

    // Write compilation related environment variables to the configuration file.
    writeln!(file, "[env]")?;
    write_env(&mut file, &["RUSTFLAGS", "CFLAGS", "CXXFLAGS"])?;

    Ok(())
}
