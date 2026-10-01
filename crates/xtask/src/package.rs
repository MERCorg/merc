use duct::cmd;
use std::env;
use std::error::Error;
use std::fs::copy;
use std::fs::create_dir_all;

/// Returns the platform-specific executable file name for a binary (adds `.exe` on Windows).
fn exe_name(binary_name: &str) -> String {
    if cfg!(windows) {
        format!("{binary_name}.exe")
    } else {
        binary_name.to_string()
    }
}

/// Builds all three workspaces (when mcrl2 and gui are set) in release mode
/// and collects the resulting binaries, the `LICENSE`, and `kahypar.ini` into a
/// `package` directory created under the current directory.
///
/// # Panics
///
/// Panics if the current directory is not a workspace root, or if a binary is
/// still missing after its `cargo build --release` succeeded.
///
/// # Errors
///
/// Returns an error if a `cargo build` invocation or a file copy fails.
pub(crate) fn package(mcrl2: bool, gui: bool) -> Result<(), Box<dyn Error>> {
    // Get the workspace root directory
    let workspace_root = env::current_dir()?;

    // Precondition: Ensure we're in a valid Rust workspace
    debug_assert!(
        workspace_root.join("Cargo.toml").exists(),
        "Must be run from workspace root containing Cargo.toml"
    );

    println!("=== Creating package directory ===");

    // Create package directory for distribution artifacts
    let package_dir = workspace_root.join("package");
    create_dir_all(&package_dir)?;

    println!("=== Building and copying release binaries ===");

    // Mapping from workspace paths to their binaries
    let mut workspace_binaries = vec![(
        workspace_root.clone(),
        vec!["merc-lts", "merc-rewrite", "merc-vpg", "merc-sym"],
    )];

    if mcrl2 {
        workspace_binaries.push((workspace_root.join("tools/mcrl2"), vec!["merc-pbes", "merc-lps"]));
    }

    if gui {
        workspace_binaries.push((workspace_root.join("tools/gui"), vec!["merc-ltsgraph"]));
    }

    // All workspaces share the root `target/` directory.
    let target_release_dir = workspace_root.join("target").join("release");

    // Build all workspaces in release mode
    for (workspace_path, binaries) in &workspace_binaries {
        cmd!("cargo", "build", "--release").dir(workspace_path).run()?;

        for binary_name in binaries {
            let source_path = target_release_dir.join(exe_name(binary_name));
            let dest_path = package_dir.join(exe_name(binary_name));

            // Precondition: Binary must exist after successful build
            assert!(
                source_path.exists(),
                "Binary {binary_name} should exist after cargo build --release"
            );

            copy(&source_path, &dest_path)?;
            println!("Copied {binary_name} to package directory");
        }
    }

    println!("=== Package creation completed ===");
    println!("Package directory: {}", package_dir.display());

    // Add the LICENSE to the package
    let license_src = workspace_root.join("LICENSE");
    let license_dest = package_dir.join("LICENSE");
    copy(&license_src, &license_dest)?;

    // Add KaHyPar configuration used by the symbolic crate
    let kahypar_ini_src = workspace_root.join("crates/symbolic/data/kahypar.ini");
    let kahypar_ini_dest = package_dir.join("kahypar.ini");
    copy(&kahypar_ini_src, &kahypar_ini_dest)?;

    Ok(())
}
