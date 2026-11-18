use std::env;
use std::error::Error;
use std::fs::create_dir_all;
use std::fs::remove_dir_all;
use std::path::Path;
use std::path::PathBuf;

use duct::cmd;
use which::which;
use which::which_in;

use crate::package::CORE_TOOLS;
use crate::package::GUI_TOOLS;
use crate::package::MCRL2_TOOLS;

/// Runs a smoke test of the packaged tool binaries to ensure that basic functionality works.
///
/// When `directory` is given, the tools are taken from that directory, which is expected to
/// be laid out as produced by `xtask package`. Otherwise, the tools are searched for on the
/// `PATH`, which can be used to test an installed package.
///
/// The `examples` directory defaults to `<directory>/../examples`, or `examples` in the
/// current directory when no `directory` is given.
pub(crate) fn test_tools(
    directory: Option<&Path>,
    examples: Option<&Path>,
    mcrl2: bool,
    gui: bool,
) -> Result<(), Box<dyn Error>> {
    let examples = match (examples, directory) {
        (Some(examples), _) => examples.to_path_buf(),
        (None, Some(directory)) => directory.join("../examples"),
        (None, None) => env::current_dir()?.join("examples"),
    };

    // Create a unique temporary directory to perform the tests in, scoped to this process so
    // concurrent or repeated runs do not clash, and clean it up afterwards.
    let tmp_path = env::temp_dir().join(format!("merc-test-tools-{}", std::process::id()));
    create_dir_all(&tmp_path)?;

    let result = run_tool_tests(directory, &examples, &tmp_path, mcrl2, gui);

    // Best-effort cleanup; do not mask the test result with a cleanup error.
    let _ = remove_dir_all(&tmp_path);

    result
}

/// Finds the given tool in `directory`, or on the `PATH` when no directory is given.
fn find_tool(tool: &str, directory: Option<&Path>) -> Result<PathBuf, Box<dyn Error>> {
    let path = match directory {
        Some(directory) => which_in(tool, Some(directory), env::current_dir()?),
        None => which(tool),
    };

    path.map_err(|err| format!("Could not find {tool}: {err}").into())
}

fn run_tool_tests(
    directory: Option<&Path>,
    examples: &Path,
    tmp_path: &Path,
    mcrl2: bool,
    gui: bool,
) -> Result<(), Box<dyn Error>> {
    let mut tools = CORE_TOOLS.to_vec();
    if mcrl2 {
        tools.extend(MCRL2_TOOLS);
    }
    if gui {
        tools.extend(GUI_TOOLS);
    }

    // Every tool must be present and start, which catches missing shared libraries.
    for tool in tools {
        let path = find_tool(tool, directory)?;
        println!("=== Checking {tool} ({}) ===", path.display());
        cmd!(&path, "--version").run()?;
    }

    // The symbolic tools look for kahypar.ini next to the (resolved) executable.
    let merc_sym = find_tool("merc-sym", directory)?.canonicalize()?;
    let kahypar_ini = merc_sym.with_file_name("kahypar.ini");
    if !kahypar_ini.exists() {
        return Err(format!("{} is missing", kahypar_ini.display()).into());
    }

    let merc_lts = find_tool("merc-lts", directory)?;

    // Copy some test files to the temporary directory.
    std::fs::copy(examples.join("lts/abp.aut"), tmp_path.join("abp.aut"))?;

    for algorithm in ["strong-bisim", "branching-bisim", "weak-bisim"] {
        println!("=== Reducing abp.aut modulo {algorithm} ===");
        cmd!(
            &merc_lts,
            "reduce",
            algorithm,
            "abp.aut",
            "--output",
            format!("abp.{algorithm}.aut")
        )
        .dir(tmp_path)
        .run()?;
    }

    Ok(())
}
