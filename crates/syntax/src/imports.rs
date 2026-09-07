//! `%import "relative/path.mcrl2"` — splicing one data specification's declarations into
//! another before type checking, using mCRL2's own comment character so a file that uses it
//! stays valid, ordinary mCRL2.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use merc_utilities::MercError;
use merc_utilities::SourceId;
use merc_utilities::SourceMap;
use merc_utilities::Span;
use merc_utilities::Spanned;

use crate::UntypedDataSpecification;

/// One `%import "relative/path"` directive, as found by [scan_imports]: the raw path text
/// between the quotes, not yet resolved against the importing file's directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDirective {
    pub path: String,
}

/// Scans `text` line by line for `%import "relative/path"` directives: a line,
/// once its leading and trailing whitespace is trimmed, of the exact shape
/// `%import "PATH"`.
///
/// Spans are local to `text`; a caller splicing multiple files together is
/// responsible for shifting them into the shared, [SourceMap]-wide offset
/// space, the same way every other span from that file's parse is.
pub fn scan_imports(text: &str) -> Vec<Spanned<ImportDirective>> {
    let mut directives = Vec::new();
    let mut offset = 0;

    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(directive) = parse_import_line(trimmed) {
            // The span covers the whole line, trailing newline excluded, so a rendered error
            // underlines the entire directive rather than just the path.
            let end = offset + line.trim_end_matches('\n').len();
            directives.push(Spanned::new(directive, Span::new(offset, end)));
        }
        offset += line.len();
    }

    directives
}

/// Recognizes one already-trimmed line as `%import "PATH"`, with no trailing content after the
/// closing quote.
fn parse_import_line(trimmed: &str) -> Option<ImportDirective> {
    let rest = trimmed.strip_prefix("%import")?;
    // Require at least one whitespace character between the keyword and the opening quote, so
    // `%importance` is not misparsed as a directive.
    let rest = rest.strip_prefix(char::is_whitespace)?.trim_start();
    let path = rest.strip_prefix('"')?.strip_suffix('"')?;

    if path.is_empty() || path.contains('"') {
        return None;
    }

    Some(ImportDirective { path: path.to_string() })
}

/// Depth-first import resolution state, threaded through one call to
/// [UntypedDataSpecification::parse_with_imports].
struct Resolver<'a> {
    sources: &'a mut SourceMap,
    /// Every file whose declarations have already been merged, by canonicalized path — so a
    /// diamond import (the same file reached from two different places in the tree) is merged
    /// once, not once per import site.
    merged: HashMap<PathBuf, SourceId>,
    /// The canonicalized paths currently being loaded, innermost last — a file reappearing in
    /// here (rather than just in `merged`) is a cycle, not a diamond.
    stack: Vec<PathBuf>,
}

impl<'a> Resolver<'a> {
    /// Starts a fresh resolution against `sources`, with nothing loaded yet.
    fn new(sources: &'a mut SourceMap) -> Self {
        Resolver {
            sources,
            merged: HashMap::new(),
            stack: Vec::new(),
        }
    }

    /// Loads `path`, merging its declarations into `output` ahead of anything
    /// `output` already holds, and returns the [SourceId] it was registered
    /// under. A file already merged earlier in this resolution is skipped
    /// rather than merged a second time.
    fn load(&mut self, path: &Path, output: &mut UntypedDataSpecification) -> Result<SourceId, MercError> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        if let Some(&id) = self.merged.get(&canonical) {
            return Ok(id);
        }

        if let Some(position) = self.stack.iter().position(|p| p == &canonical) {
            let cycle = self.stack[position..]
                .iter()
                .chain(std::iter::once(&canonical))
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n  imports ");
            return Err(format!("import cycle detected:\n  {cycle}").into());
        }

        let source_id = self.sources.load_file(path)?;
        // The file's text is registered — and so its base offset into the shared, global byte
        // space fixed — *before* it (or anything it imports) is parsed, which is what lets the
        // padding trick below stand in for a per-node span-rebasing pass.
        let base = self.sources.base_offset(source_id);
        let text = self.sources.text(source_id).to_string();

        self.stack.push(canonical.clone());

        let directory = path.parent().unwrap_or_else(|| Path::new("."));
        for directive in scan_imports(&text) {
            let import_path = directory.join(&directive.node.path);
            self.load(&import_path, output).map_err(|error| {
                let span = Span::new(base + directive.span.start, base + directive.span.end);
                format!("{error}\n{}", span.render(self.sources))
            })?;
        }

        // Padding `text` with `base` leading spaces before parsing makes every byte offset pest
        // reports already correct in the shared, global space.
        let padded = " ".repeat(base) + &text;
        let file_spec = UntypedDataSpecification::parse(&padded)
            .map_err(|error| format!("in {}:\n{error}", path.display()))?;
        output.merge(&file_spec);

        self.stack.pop();
        self.merged.insert(canonical, source_id);

        Ok(source_id)
    }
}

impl UntypedDataSpecification {
    /// Parses `root_path` and every data specification it (transitively)
    /// `%import`s, merging them all into one [UntypedDataSpecification].
    /// 
    /// Every declaration keeps a [merc_utilities::Span] that renders correctly
    /// (see [merc_utilities::Span::render]) against the returned `sources`,
    /// whether it came from `root_path` or from something it imported.
    ///
    /// `sources` accumulates every file loaded this way; pass a fresh, empty
    /// [SourceMap] for a single call, or reuse one across several
    /// `parse_with_imports` calls so a file imported by more than one of them
    /// is still only loaded once.
    pub fn parse_with_imports(
        root_path: &Path,
        sources: &mut SourceMap,
    ) -> Result<(UntypedDataSpecification, SourceId), MercError> {
        let mut resolver = Resolver::new(sources);
        let mut output = UntypedDataSpecification::default();
        let root_id = resolver.load(root_path, &mut output)?;
        Ok((output, root_id))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use merc_utilities::SourceMap;

    use super::*;

    #[test]
    fn test_scan_imports_finds_a_directive_line() {
        let text = "sort D;\n%import \"other.mcrl2\"\nmap f: D;\n";
        let directives = scan_imports(text);

        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].node.path, "other.mcrl2");
    }

    #[test]
    fn test_scan_imports_ignores_ordinary_comments() {
        let text = "% just a comment\n%importance is not a directive\nsort D;\n";
        assert!(scan_imports(text).is_empty());
    }

    #[test]
    fn test_scan_imports_ignores_a_directive_with_trailing_garbage() {
        assert!(scan_imports("%import \"a.mcrl2\" extra\n").is_empty());
    }

    #[test]
    fn test_scan_imports_allows_leading_whitespace() {
        let directives = scan_imports("   %import \"a.mcrl2\"\n");
        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].node.path, "a.mcrl2");
    }

    /// Writes `files` (relative-path -> contents) into a fresh temp directory and returns it.
    fn temp_project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("should create a temp directory");
        for (name, contents) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("should create parent directories");
            }
            fs::write(path, contents).expect("should write the fixture file");
        }
        dir
    }

    #[test]
    fn test_parse_with_imports_merges_the_imported_declarations() {
        let dir = temp_project(&[
            ("main.mcrl2", "%import \"common.mcrl2\"\nmap g: D;\n"),
            ("common.mcrl2", "sort D;\n"),
        ]);

        let mut sources = SourceMap::new();
        let (spec, _root_id) =
            UntypedDataSpecification::parse_with_imports(&dir.path().join("main.mcrl2"), &mut sources)
                .expect("should resolve the import");

        assert_eq!(spec.sort_declarations.len(), 1);
        assert_eq!(spec.map_declarations.len(), 1);
    }

    #[test]
    fn test_parse_with_imports_gives_every_declaration_a_span_rendering_against_its_own_file() {
        let dir = temp_project(&[
            ("main.mcrl2", "%import \"common.mcrl2\"\nmap g: D;\n"),
            ("common.mcrl2", "sort D;\n"),
        ]);

        let mut sources = SourceMap::new();
        let (spec, _root_id) =
            UntypedDataSpecification::parse_with_imports(&dir.path().join("main.mcrl2"), &mut sources)
                .expect("should resolve the import");

        // The imported sort declaration's span must render against `common.mcrl2`, not `main.mcrl2`.
        let sort_span = &spec.sort_declarations[0].span;
        let rendered = sort_span.render(&sources);
        assert!(
            rendered.contains("common.mcrl2"),
            "expected the sort declaration to render against common.mcrl2, got: {rendered}"
        );
    }

    #[test]
    fn test_parse_with_imports_merges_a_diamond_import_once() {
        let dir = temp_project(&[
            ("main.mcrl2", "%import \"a.mcrl2\"\n%import \"b.mcrl2\"\n"),
            ("a.mcrl2", "%import \"common.mcrl2\"\n"),
            ("b.mcrl2", "%import \"common.mcrl2\"\n"),
            ("common.mcrl2", "sort D;\n"),
        ]);

        let mut sources = SourceMap::new();
        let (spec, _root_id) =
            UntypedDataSpecification::parse_with_imports(&dir.path().join("main.mcrl2"), &mut sources)
                .expect("should resolve the diamond import");

        assert_eq!(spec.sort_declarations.len(), 1);
    }

    #[test]
    fn test_parse_with_imports_rejects_a_cycle() {
        let dir = temp_project(&[
            ("a.mcrl2", "%import \"b.mcrl2\"\n"),
            ("b.mcrl2", "%import \"a.mcrl2\"\n"),
        ]);

        let mut sources = SourceMap::new();
        let error = UntypedDataSpecification::parse_with_imports(&dir.path().join("a.mcrl2"), &mut sources)
            .expect_err("a cyclic import must be rejected");

        assert!(
            error.to_string().contains("import cycle detected"),
            "got: {error}"
        );
    }

    #[test]
    fn test_parse_with_imports_rejects_a_self_import() {
        let dir = temp_project(&[("a.mcrl2", "%import \"a.mcrl2\"\n")]);

        let mut sources = SourceMap::new();
        let error = UntypedDataSpecification::parse_with_imports(&dir.path().join("a.mcrl2"), &mut sources)
            .expect_err("a file importing itself must be rejected");

        assert!(
            error.to_string().contains("import cycle detected"),
            "got: {error}"
        );
    }

    #[test]
    fn test_parse_with_imports_reports_a_missing_import() {
        let dir = temp_project(&[("main.mcrl2", "%import \"missing.mcrl2\"\n")]);

        let mut sources = SourceMap::new();
        let error = UntypedDataSpecification::parse_with_imports(&dir.path().join("main.mcrl2"), &mut sources);

        assert!(error.is_err());
    }
}
