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
use crate::UntypedProcessSpecification;
use crate::UntypedStateFrmSpec;

/// One `%import "relative/path"` directive, as found by [scan_imports]: the raw path text
/// between the quotes, not yet resolved against the importing file's directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDirective {
    pub path: String,
    /// Span of just the quoted path text (excluding the quotes themselves).
    pub path_span: Span,
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
        let leading_whitespace = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if let Some(directive) = parse_import_line(trimmed, offset + leading_whitespace) {
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
/// closing quote. `base` is `trimmed`'s own offset within the file being scanned, used to give
/// [ImportDirective::path_span] an absolute (file-local) span rather than one relative to
/// `trimmed`.
fn parse_import_line(trimmed: &str, base: usize) -> Option<ImportDirective> {
    let rest = trimmed.strip_prefix("%import")?;
    // Require at least one whitespace character between the keyword and the opening quote, so
    // `%importance` is not misparsed as a directive.
    let rest = rest.strip_prefix(char::is_whitespace)?.trim_start();
    let path = rest.strip_prefix('"')?.strip_suffix('"')?;

    if path.is_empty() || path.contains('"') {
        return None;
    }

    // `rest` still ends exactly where `trimmed` does, so its start is the byte offset of the
    // opening quote within `trimmed`; the path text itself starts one byte past that.
    let quote_offset = trimmed.len() - rest.len();
    let path_start = base + quote_offset + 1;
    let path_end = path_start + path.len();

    Some(ImportDirective {
        path: path.to_string(),
        path_span: Span::new(path_start, path_end),
    })
}

/// Implemented by every untyped AST that `%import` can compose.
trait ImportMergeable: Sized {
    /// Parses one file's complete, already-padded text as this type.
    fn parse_padded(text: &str) -> Result<Self, MercError>;

    /// Merges an *imported* file's declarations into `self`, ahead of anything `self` already
    /// holds. Used for every file reached via a `%import` directive, however deeply nested.
    fn merge_imported(&mut self, other: &Self);

    /// As [Self::merge_imported], but for the root file's own parse.
    /// 
    /// Can be used to merge the root file's own declarations in the same way as
    /// imported files.
    fn merge_own(&mut self, other: &Self) {
        self.merge_imported(other);
    }
}

impl ImportMergeable for UntypedDataSpecification {
    fn parse_padded(text: &str) -> Result<Self, MercError> {
        UntypedDataSpecification::parse(text)
    }

    fn merge_imported(&mut self, other: &Self) {
        self.merge(other);
    }
}

impl ImportMergeable for UntypedProcessSpecification {
    fn parse_padded(text: &str) -> Result<Self, MercError> {
        UntypedProcessSpecification::parse(text)
    }

    fn merge_imported(&mut self, other: &Self) {
        self.merge(other);
    }

    fn merge_own(&mut self, other: &Self) {
        self.merge(other);
        self.init = other.init.clone();
    }
}

/// Depth-first import resolution state, threaded through one call to
/// [UntypedDataSpecification::parse_with_imports] (or another type's own `parse_with_imports`).
struct Resolver<'a, T> {
    sources: &'a mut SourceMap,
    /// Every file whose declarations have already been merged, by canonicalized path — so a
    /// diamond import (the same file reached from two different places in the tree) is merged
    /// once, not once per import site.
    merged: HashMap<PathBuf, SourceId>,
    /// The canonicalized paths currently being loaded, innermost last — a file reappearing in
    /// here (rather than just in `merged`) is a cycle, not a diamond.
    stack: Vec<PathBuf>,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T: ImportMergeable> Resolver<'a, T> {
    /// Starts a fresh resolution against `sources`, with nothing loaded yet.
    fn new(sources: &'a mut SourceMap) -> Self {
        Resolver {
            sources,
            merged: HashMap::new(),
            stack: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }

    /// As [Self::load_with_text], reading `path` from disk rather than being handed its text.
    fn load(&mut self, path: &Path, output: &mut T) -> Result<SourceId, MercError> {
        self.load_with_text(path, None, output)
    }

    /// Loads `path`, merging its declarations into `output` ahead of anything
    /// `output` already holds, and returns the [SourceId] it was registered
    /// under. A file already merged earlier in this resolution is skipped
    /// rather than merged a second time.
    ///
    /// `text_override`, when given, is used as `path`'s own text instead of reading `path` from
    /// disk.
    fn load_with_text(&mut self, path: &Path, text_override: Option<&str>, output: &mut T) -> Result<SourceId, MercError> {
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

        // A file is the *root* of this resolution exactly when nothing is on the stack yet.
        let is_root = self.stack.is_empty();

        let source_id = match text_override {
            Some(text) => self.sources.add_text(path.display().to_string(), text.to_string()),
            None => self.sources.load_file(path)?,
        };
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
        let file_spec = T::parse_padded(&padded).map_err(|error| format!("in {}:\n{error}", path.display()))?;
        if is_root {
            output.merge_own(&file_spec);
        } else {
            output.merge_imported(&file_spec);
        }

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
        let mut resolver: Resolver<UntypedDataSpecification> = Resolver::new(sources);
        let mut output = UntypedDataSpecification::default();
        let root_id = resolver.load(root_path, &mut output)?;
        Ok((output, root_id))
    }
}

impl UntypedProcessSpecification {
    /// As [UntypedDataSpecification::parse_with_imports], for a process specification: `root_path`
    /// and every process (or data) specification it (transitively) `%import`s are parsed and
    /// merged into one [UntypedProcessSpecification], with the same span/`sources`/cycle/diamond
    /// guarantees.
    /// 
    /// `text` is `root_path`'s own text, used as-is rather than re-read from disk.
    pub fn parse_with_imports(
        root_path: &Path,
        text: &str,
        sources: &mut SourceMap,
    ) -> Result<(UntypedProcessSpecification, SourceId), MercError> {
        let mut resolver: Resolver<UntypedProcessSpecification> = Resolver::new(sources);
        let mut output = UntypedProcessSpecification::default();
        let root_id = resolver.load_with_text(root_path, Some(text), &mut output)?;
        Ok((output, root_id))
    }
}

impl UntypedStateFrmSpec {
    /// Parses `root_path` as a modal (mu-calculus) state-formula specification,
    /// resolving every `%import` directive found in its own text against
    /// process specifications rather than other formula files.
    ///
    /// `text` is `root_path`'s own text, used as-is rather than re-read from
    /// disk.
    pub fn parse_with_imports(root_path: &Path, text: &str, sources: &mut SourceMap) -> Result<(UntypedStateFrmSpec, SourceId), MercError> {
        let root_id = sources.add_text(root_path.display().to_string(), text.to_string());
        // Registered (and so base-offset-fixed) before anything it imports is parsed, same
        // padding-trick precondition `Resolver::load` relies on for every other file kind.
        let base = sources.base_offset(root_id);
        let text = sources.text(root_id).to_string();

        let directory = root_path.parent().unwrap_or_else(|| Path::new("."));
        let mut resolver: Resolver<UntypedProcessSpecification> = Resolver::new(sources);
        let mut imported = UntypedProcessSpecification::default();
        for directive in scan_imports(&text) {
            let import_path = directory.join(&directive.node.path);
            resolver.load(&import_path, &mut imported).map_err(|error| {
                let span = Span::new(base + directive.span.start, base + directive.span.end);
                format!("{error}\n{}", span.render(resolver.sources))
            })?;
        }

        let padded = " ".repeat(base) + &text;
        let mut spec = UntypedStateFrmSpec::parse(&padded).map_err(|error| format!("in {}:\n{error}", root_path.display()))?;

        // `imported` was built the same way `Resolver::load` builds up a file's own accumulator.
        imported.data_specification.merge(&spec.data_specification);
        imported.action_declarations.extend_from_slice(&spec.action_declarations);
        spec.data_specification = imported.data_specification;
        spec.action_declarations = imported.action_declarations;

        Ok((spec, root_id))
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

    #[test]
    fn test_scan_imports_path_span_covers_just_the_quoted_path() {
        let text = "%import \"a.mcrl2\"\n";
        let directives = scan_imports(text);

        assert_eq!(directives.len(), 1);
        let path_span = &directives[0].node.path_span;
        assert_eq!(&text[path_span.start..path_span.end], "a.mcrl2");
    }

    #[test]
    fn test_process_spec_parse_with_imports_merges_the_imported_declarations() {
        let main_text = "%import \"common.mcrl2\"\nact b: D;\ninit a(c) . b(c);\n";
        let dir = temp_project(&[
            ("main.mcrl2", main_text),
            ("common.mcrl2", "sort D;\ncons c: D;\nact a: D;\n"),
        ]);

        let mut sources = SourceMap::new();
        let (spec, _root_id) =
            UntypedProcessSpecification::parse_with_imports(&dir.path().join("main.mcrl2"), main_text, &mut sources)
                .expect("should resolve the import");

        assert_eq!(spec.data_specification.sort_declarations.len(), 1);
        assert_eq!(spec.data_specification.constructor_declarations.len(), 1);
        assert_eq!(spec.action_declarations.len(), 2);
        assert!(spec.init.is_some());
    }

    #[test]
    fn test_process_spec_parse_with_imports_gives_every_declaration_a_span_rendering_against_its_own_file() {
        let main_text = "%import \"common.mcrl2\"\ninit a;\n";
        let dir = temp_project(&[
            ("main.mcrl2", main_text),
            ("common.mcrl2", "act a;\n"),
        ]);

        let mut sources = SourceMap::new();
        let (spec, _root_id) =
            UntypedProcessSpecification::parse_with_imports(&dir.path().join("main.mcrl2"), main_text, &mut sources)
                .expect("should resolve the import");

        let action_span = &spec.action_declarations[0].identifier.span;
        let rendered = action_span.render(&sources);
        assert!(
            rendered.contains("common.mcrl2"),
            "expected the act declaration to render against common.mcrl2, got: {rendered}"
        );
    }

    #[test]
    fn test_process_spec_parse_with_imports_keeps_the_importing_files_own_init() {
        // A file being imported is free to carry an `init` of its own (`MCRL2Spec`'s `Init` is
        // optional either way) — it must never override the importing file's own.
        let main_text = "%import \"common.mcrl2\"\nact b;\ninit b;\n";
        let dir = temp_project(&[
            ("main.mcrl2", main_text),
            ("common.mcrl2", "act a;\ninit a;\n"),
        ]);

        let mut sources = SourceMap::new();
        let (spec, _root_id) =
            UntypedProcessSpecification::parse_with_imports(&dir.path().join("main.mcrl2"), main_text, &mut sources)
                .expect("should resolve the import");

        assert_eq!(spec.action_declarations.len(), 2);
        let init = spec.init.expect("main.mcrl2's own init must survive");
        // `init b;` — not `common.mcrl2`'s `init a;`.
        assert!(format!("{init:?}").contains('b'));
    }

    #[test]
    fn test_modal_spec_parse_with_imports_pulls_in_action_declarations() {
        let formula_text = "%import \"common.mcrl2\"\nform nu X . [a]X;\n";
        let dir = temp_project(&[
            ("formula.mcf", formula_text),
            ("common.mcrl2", "act a;\n"),
        ]);

        let mut sources = SourceMap::new();
        let (spec, _root_id) =
            UntypedStateFrmSpec::parse_with_imports(&dir.path().join("formula.mcf"), formula_text, &mut sources)
                .expect("should resolve the import");

        assert_eq!(spec.action_declarations.len(), 1);
        assert_eq!(spec.action_declarations[0].identifier.node, "a");
    }

    #[test]
    fn test_modal_spec_parse_with_imports_gives_the_imported_action_a_span_rendering_against_its_own_file() {
        let formula_text = "%import \"common.mcrl2\"\nform nu X . [a]X;\n";
        let dir = temp_project(&[
            ("formula.mcf", formula_text),
            ("common.mcrl2", "act a;\n"),
        ]);

        let mut sources = SourceMap::new();
        let (spec, _root_id) =
            UntypedStateFrmSpec::parse_with_imports(&dir.path().join("formula.mcf"), formula_text, &mut sources)
                .expect("should resolve the import");

        let action_span = &spec.action_declarations[0].identifier.span;
        let rendered = action_span.render(&sources);
        assert!(
            rendered.contains("common.mcrl2"),
            "expected the act declaration to render against common.mcrl2, got: {rendered}"
        );
    }

    #[test]
    fn test_modal_spec_parse_with_imports_reports_a_missing_import() {
        let formula_text = "%import \"missing.mcrl2\"\nform true;\n";
        let dir = temp_project(&[("formula.mcf", formula_text)]);

        let mut sources = SourceMap::new();
        let error = UntypedStateFrmSpec::parse_with_imports(&dir.path().join("formula.mcf"), formula_text, &mut sources);

        assert!(error.is_err());
    }
}
