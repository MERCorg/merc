use duct::cmd;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::env;
use std::error::Error;
use std::fs;
use std::fs::copy;
use std::fs::create_dir_all;
use std::path::Path;
use std::path::PathBuf;

/// The name of the installed package.
const NAME: &str = "merc";
/// The version of the package, taken from the workspace version.
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The SPDX license identifier, taken from the workspace license.
const LICENSE: &str = env!("CARGO_PKG_LICENSE");
/// The project homepage, taken from the workspace homepage.
const HOMEPAGE: &str = env!("CARGO_PKG_HOMEPAGE");
const MAINTAINER: &str = "MERCorg";
const SUMMARY: &str = "Generic libraries and tools for model checking";
const DESCRIPTION: &str = "mCRL2 except Reliable & Concurrent (merc) provides a generic set of \
    libraries and tools for (specification language-agnostic) model checking.";

/// The directory where the deb and rpm packages install all files. The binaries must
/// live next to `kahypar.ini`, so they are kept together and symlinked into `/usr/bin`.
const UNIX_INSTALL_DIR: &str = "/opt/merc";

/// Fixed identifier that lets Windows Installer upgrade earlier versions in place. Never change it.
const MSI_UPGRADE_CODE: &str = "B6D80BFA-73EA-40DF-A65B-081294691553";

/// Template for the Debian `control` file.
const CONTROL_TEMPLATE: &str = include_str!("../data/control");
/// Template for the WiX source file used to build the `.msi`.
const WXS_TEMPLATE: &str = include_str!("../data/merc.wxs");

/// The tools built from the root workspace.
pub(crate) const CORE_TOOLS: &[&str] = &["merc-lts", "merc-rewrite", "merc-vpg", "merc-sym"];
/// The tools built from the `tools/mcrl2` workspace, these depend on the mCRL2 toolset.
pub(crate) const MCRL2_TOOLS: &[&str] = &["merc-pbes", "merc-lps"];
/// The tools built from the `tools/gui` workspace.
pub(crate) const GUI_TOOLS: &[&str] = &["merc-ltsgraph"];

/// The installer formats that can be created from the package directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Installer {
    /// Debian package, built in pure Rust.
    Deb,
    /// RPM package, built in pure Rust.
    Rpm,
    /// macOS disk image, requires `hdiutil` (macOS only).
    Dmg,
    /// Windows installer, requires the WiX toolset (`wix`) v4 or later.
    Msi,
}

/// The files collected in the package directory.
struct PackageContents {
    /// The directory containing all files.
    dir: PathBuf,
    /// File names of the executables in `dir`.
    binaries: Vec<String>,
    /// File names of the other (non-executable) files in `dir`.
    data_files: Vec<String>,
}

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
/// `package` directory created under the current directory. Afterwards, the requested
/// installers are written to an `installers` directory under the current directory.
///
/// # Panics
///
/// Panics if the current directory is not a workspace root, or if a binary is
/// still missing after its `cargo build --release` succeeded.
///
/// # Errors
///
/// Returns an error if a `cargo build` invocation, a file copy, or creating an installer fails.
pub(crate) fn package(mcrl2: bool, gui: bool, installers: &[Installer]) -> Result<(), Box<dyn Error>> {
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
    let mut workspace_binaries = vec![(workspace_root.clone(), CORE_TOOLS)];

    if mcrl2 {
        workspace_binaries.push((workspace_root.join("tools/mcrl2"), MCRL2_TOOLS));
    }

    if gui {
        workspace_binaries.push((workspace_root.join("tools/gui"), GUI_TOOLS));
    }

    // All workspaces share the root `target/` directory.
    let target_release_dir = workspace_root.join("target").join("release");

    let mut binaries = Vec::new();

    // Cargo only tells build scripts whether debug info is enabled, so `cc` compiles C and C++
    // dependencies (most notably mCRL2) with full `-g` even though the release profile asks for
    // line tables only. Appending `-g1` lowers that to line tables, which still give backtraces.
    let mut build = cmd!("cargo", "build", "--release");
    if !cfg!(target_env = "msvc") {
        for flags in ["CFLAGS", "CXXFLAGS"] {
            let value = env::var(flags).map_or_else(|_| "-g1".to_string(), |value| format!("{value} -g1"));
            build = build.env(flags, value);
        }
    }

    // Build all workspaces in release mode
    for (workspace_path, workspace_bins) in &workspace_binaries {
        build.dir(workspace_path).run()?;

        for binary_name in workspace_bins.iter() {
            let file_name = exe_name(binary_name);
            let source_path = target_release_dir.join(&file_name);
            let dest_path = package_dir.join(&file_name);

            // Precondition: Binary must exist after successful build
            assert!(
                source_path.exists(),
                "Binary {binary_name} should exist after cargo build --release"
            );

            copy(&source_path, &dest_path)?;
            println!("Copied {binary_name} to package directory");

            // Only ELF binaries embed their debug info, on macOS and Windows it is kept in
            // separate files. Compressed sections are still read by the backtrace symbolizer.
            if cfg!(target_os = "linux") {
                cmd!("objcopy", "--compress-debug-sections=zlib", &dest_path).run()?;
            }
            binaries.push(file_name);
        }
    }

    // Add the LICENSE to the package
    copy(workspace_root.join("LICENSE"), package_dir.join("LICENSE"))?;

    // Add KaHyPar configuration used by the symbolic crate
    copy(
        workspace_root.join("crates/transition_systems/symbolic/data/kahypar.ini"),
        package_dir.join("kahypar.ini"),
    )?;

    println!("=== Package creation completed ===");
    println!("Package directory: {}", package_dir.display());

    let contents = PackageContents {
        dir: package_dir,
        binaries,
        data_files: vec!["LICENSE".to_string(), "kahypar.ini".to_string()],
    };

    if !installers.is_empty() {
        let installer_dir = workspace_root.join("installers");
        create_dir_all(&installer_dir)?;

        for installer in installers {
            println!("=== Building {installer:?} installer ===");
            let path = match installer {
                Installer::Deb => build_deb(&contents, &installer_dir)?,
                Installer::Rpm => build_rpm(&contents, &installer_dir)?,
                Installer::Dmg => build_dmg(&contents, &installer_dir)?,
                Installer::Msi => build_msi(&contents, &installer_dir)?,
            };
            println!("Created {}", path.display());
        }
    }

    Ok(())
}

/// Renders the given template with the given values.
fn render_template(template: &str, values: upon::Value) -> Result<String, upon::Error> {
    let engine = upon::Engine::new();
    engine.compile(template)?.render(&engine, values).to_string()
}

/// Returns the Debian architecture name of the host, e.g., `amd64`.
fn deb_arch() -> &'static str {
    match env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "i386",
        "arm" => "armhf",
        other => other,
    }
}

/// Appends a file with the given contents and permissions to a tar archive, owned by root.
fn tar_append_file<W: std::io::Write>(
    builder: &mut tar::Builder<W>,
    path: &str,
    contents: &[u8],
    mode: u32,
) -> std::io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(contents.len() as u64);
    header.set_mode(mode);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    builder.append_data(&mut header, path, contents)
}

/// Appends a directory entry to a tar archive, owned by root.
fn tar_append_dir<W: std::io::Write>(builder: &mut tar::Builder<W>, path: &str) -> std::io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Directory);
    header.set_size(0);
    header.set_mode(0o755);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    builder.append_data(&mut header, path, std::io::empty())
}

/// Appends a symbolic link `path -> target` to a tar archive, owned by root.
fn tar_append_symlink<W: std::io::Write>(
    builder: &mut tar::Builder<W>,
    path: &str,
    target: &str,
) -> std::io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o777);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    builder.append_link(&mut header, path, target)
}

/// Finishes a gzip-compressed tar archive and returns its bytes.
fn finish_tar_gz(builder: tar::Builder<GzEncoder<Vec<u8>>>) -> std::io::Result<Vec<u8>> {
    builder.into_inner()?.finish()
}

/// Returns true when the graphical tool is part of the package.
fn has_gui(contents: &PackageContents) -> bool {
    contents.binaries.iter().any(|binary| binary == "merc-ltsgraph")
}

/// The Debian packages that the binaries dynamically link against.
fn deb_depends(contents: &PackageContents) -> Vec<&'static str> {
    let mut depends = vec!["libc6", "libgcc-s1", "libstdc++6"];
    if has_gui(contents) {
        depends.push("libfontconfig1");
    }
    depends
}

/// The RPM packages that the binaries dynamically link against, glibc is always present.
fn rpm_requires(contents: &PackageContents) -> Vec<&'static str> {
    let mut requires = vec!["libgcc", "libstdc++"];
    if has_gui(contents) {
        requires.push("fontconfig");
    }
    requires
}

/// Builds a Debian package (`.deb`) that installs everything into [`UNIX_INSTALL_DIR`] and
/// symlinks the binaries into `/usr/bin`. This is done in pure Rust, so it works on any host,
/// but the binaries must have been built for Linux.
///
/// A `.deb` is an `ar` archive containing `debian-binary`, `control.tar.gz` and `data.tar.gz`.
fn build_deb(contents: &PackageContents, output_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    // Build the data archive with the installed files.
    let mut data = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::best()));
    let mut installed_size = 0u64;

    // Paths in the archives are relative to the root of the target system.
    let install_dir = UNIX_INSTALL_DIR.trim_start_matches('/');
    let bin_dir = format!("{install_dir}/bin");
    let doc_dir = format!("usr/share/doc/{NAME}");
    for dir in [
        "opt",
        install_dir,
        &bin_dir,
        "usr",
        "usr/bin",
        "usr/share",
        "usr/share/doc",
        &doc_dir,
    ] {
        tar_append_dir(&mut data, dir)?;
    }

    for binary in &contents.binaries {
        let bytes = fs::read(contents.dir.join(binary))?;
        installed_size += bytes.len() as u64;
        tar_append_file(&mut data, &format!("{bin_dir}/{binary}"), &bytes, 0o755)?;
        tar_append_symlink(
            &mut data,
            &format!("usr/bin/{binary}"),
            &format!("{UNIX_INSTALL_DIR}/bin/{binary}"),
        )?;
    }

    for file in &contents.data_files {
        let bytes = fs::read(contents.dir.join(file))?;
        installed_size += bytes.len() as u64;
        tar_append_file(&mut data, &format!("{bin_dir}/{file}"), &bytes, 0o644)?;
    }

    // Debian policy requires the license in /usr/share/doc/<package>/copyright.
    let license = fs::read(contents.dir.join("LICENSE"))?;
    tar_append_file(&mut data, &format!("{doc_dir}/copyright"), &license, 0o644)?;

    let data = finish_tar_gz(data)?;

    // Build the control archive with the package metadata.
    let control = render_template(
        CONTROL_TEMPLATE,
        upon::value! {
            name: NAME,
            version: VERSION,
            arch: deb_arch(),
            maintainer: MAINTAINER,
            installed_size: installed_size.div_ceil(1024),
            depends: deb_depends(contents).join(", "),
            homepage: HOMEPAGE,
            summary: SUMMARY,
            description: DESCRIPTION,
        },
    )?;

    let mut control_tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::best()));
    tar_append_file(&mut control_tar, "control", control.as_bytes(), 0o644)?;
    let control_tar = finish_tar_gz(control_tar)?;

    // Combine everything into the ar archive, the order of the members is mandatory.
    let path = output_dir.join(format!("{NAME}_{VERSION}_{}.deb", deb_arch()));
    let mut deb = ar::Builder::new(fs::File::create(&path)?);
    for (name, bytes) in [
        ("debian-binary", b"2.0\n".as_slice()),
        ("control.tar.gz", &control_tar),
        ("data.tar.gz", &data),
    ] {
        let mut header = ar::Header::new(name.as_bytes().to_vec(), bytes.len() as u64);
        header.set_mode(0o100644);
        deb.append(&header, bytes)?;
    }

    Ok(path)
}

/// Builds an RPM package (`.rpm`) using the `rpm` crate, with the same layout as [`build_deb`].
fn build_rpm(contents: &PackageContents, output_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let arch = env::consts::ARCH;
    let mut builder = rpm::PackageBuilder::new(NAME, VERSION, LICENSE, arch, SUMMARY);
    builder
        .release("1")
        .url(HOMEPAGE)
        .vendor(MAINTAINER)
        .description(DESCRIPTION);

    for dependency in rpm_requires(contents) {
        builder.requires(rpm::Dependency::any(dependency));
    }

    for binary in &contents.binaries {
        let dest = format!("{UNIX_INSTALL_DIR}/bin/{binary}");
        builder.with_file(
            contents.dir.join(binary),
            rpm::FileOptions::new(&dest).permissions(0o755),
        )?;
        builder.with_symlink(rpm::FileOptions::symlink(format!("/usr/bin/{binary}"), dest))?;
    }

    for file in &contents.data_files {
        let options = rpm::FileOptions::new(format!("{UNIX_INSTALL_DIR}/bin/{file}")).permissions(0o644);
        let options = if file == "LICENSE" { options.license() } else { options };
        builder.with_file(contents.dir.join(file), options)?;
    }

    let path = output_dir.join(format!("{NAME}-{VERSION}-1.{arch}.rpm"));
    builder.build()?.write_file(&path)?;
    Ok(path)
}

/// Builds a compressed macOS disk image (`.dmg`) containing the package directory using `hdiutil`.
fn build_dmg(contents: &PackageContents, output_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    if !cfg!(target_os = "macos") {
        return Err("Creating a .dmg requires hdiutil, which is only available on macOS".into());
    }

    let path = output_dir.join(format!("{NAME}-{VERSION}-{}.dmg", env::consts::ARCH));
    cmd!(
        "hdiutil",
        "create",
        "-volname",
        format!("{NAME} {VERSION}"),
        "-srcfolder",
        &contents.dir,
        "-ov",
        "-format",
        "UDZO",
        &path
    )
    .run()?;

    Ok(path)
}

/// Escapes a string for use inside an XML attribute.
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Builds a Windows installer (`.msi`) that installs into `Program Files\merc` and adds that
/// directory to the system `PATH`. When `merc-ltsgraph` is included, the installer also creates
/// a Start Menu shortcut and offers an optional desktop shortcut. Requires WiX and its UI
/// extension, which can be installed with `dotnet tool install --global wix` and
/// `wix extension add -g WixToolset.UI.wixext`.
fn build_msi(contents: &PackageContents, output_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    if which::which("wix").is_err() {
        return Err(
            "Creating a .msi requires the WiX toolset and UI extension; install them with `dotnet tool install --global wix` and `wix extension add -g WixToolset.UI.wixext`".into(),
        );
    }

    let license_rtf = output_dir.join(format!("{NAME}-license.rtf"));
    fs::write(&license_rtf, license_to_rtf(include_str!("../../../LICENSE")))?;
    let license_rtf = license_rtf.to_string_lossy().into_owned();
    let wxs = render_msi_source(contents, &license_rtf)?;

    let wxs_path = output_dir.join(format!("{NAME}.wxs"));
    fs::write(&wxs_path, wxs)?;

    let arch = match env::consts::ARCH {
        "aarch64" => "arm64",
        "x86" => "x86",
        _ => "x64",
    };
    let path = output_dir.join(format!("{NAME}-{VERSION}-{arch}.msi"));
    cmd!(
        "wix",
        "build",
        "-ext",
        "WixToolset.UI.wixext",
        "-arch",
        arch,
        "-o",
        &path,
        &wxs_path
    )
    .run()?;

    Ok(path)
}

fn render_msi_source(contents: &PackageContents, license_rtf: &str) -> Result<String, upon::Error> {
    let ltsgraph_binary = exe_name("merc-ltsgraph");
    let gui = contents.binaries.iter().any(|binary| binary == &ltsgraph_binary);
    let files: Vec<String> = contents
        .binaries
        .iter()
        .filter(|binary| *binary != &ltsgraph_binary)
        .chain(&contents.data_files)
        .map(|file| xml_escape(&contents.dir.join(file).to_string_lossy()))
        .collect();
    let ltsgraph = xml_escape(&contents.dir.join(ltsgraph_binary).to_string_lossy());

    render_template(
        WXS_TEMPLATE,
        upon::value! {
            name: NAME,
            version: VERSION,
            maintainer: MAINTAINER,
            upgrade_code: MSI_UPGRADE_CODE,
            files: files,
            gui: gui,
            ltsgraph: ltsgraph,
            license_rtf: xml_escape(license_rtf),
        },
    )
}

/// Converts plain-text license content to RTF for WiX's built-in license dialog.
fn license_to_rtf(license: &str) -> String {
    let mut escaped = String::with_capacity(license.len());
    for character in license.chars() {
        match character {
            '\\' => escaped.push_str(r"\\"),
            '{' => escaped.push_str(r"\{"),
            '}' => escaped.push_str(r"\}"),
            '\r' => {}
            '\n' => escaped.push_str(r"\par "),
            character if character.is_ascii() => escaped.push(character),
            character => {
                let mut utf16 = [0; 2];
                for code_unit in character.encode_utf16(&mut utf16) {
                    escaped.push_str(&format!(r"\u{}?", *code_unit as i16));
                }
            }
        }
    }

    format!(r"{{\rtf1\ansi\deff0{{\fonttbl{{\f0\fnil Arial;}}}}\viewkind4\uc1\pard\f0\fs20 {escaped}\par}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msi_source_offers_gui_shortcuts_only_when_the_gui_binary_is_present() {
        let contents = PackageContents {
            dir: PathBuf::from("package"),
            binaries: vec![exe_name("merc-lts"), exe_name("merc-ltsgraph")],
            data_files: vec!["LICENSE".to_string()],
        };
        let source = render_msi_source(&contents, "merc-license.rtf").unwrap();

        assert!(source.contains("WixUI_FeatureTree"));
        assert!(source.contains("MercLtsgraphExe"));
        assert!(source.contains("LtsgraphStartMenuShortcut"));
        assert!(source.contains("LtsgraphDesktopShortcut"));
        assert!(source.contains("Level=\"2\""));
        assert!(source.contains("merc-lts.exe"));

        let contents = PackageContents {
            dir: PathBuf::from("package"),
            binaries: vec![exe_name("merc-lts")],
            data_files: vec!["LICENSE".to_string()],
        };
        let source = render_msi_source(&contents, "merc-license.rtf").unwrap();

        assert!(!source.contains("MercLtsgraphExe"));
        assert!(!source.contains("LtsgraphStartMenuShortcut"));
        assert!(!source.contains("LtsgraphDesktopShortcut"));
        assert!(source.contains("merc-lts.exe"));
    }

    #[test]
    fn license_rtf_escapes_control_characters_and_preserves_text() {
        assert_eq!(
            license_to_rtf("Terms {of} use\\back\nslash"),
            r"{\rtf1\ansi\deff0{\fonttbl{\f0\fnil Arial;}}\viewkind4\uc1\pard\f0\fs20 Terms \{of\} use\\back\par slash\par}"
        );
    }
}
